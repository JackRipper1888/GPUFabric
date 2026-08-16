# gpuf-s Test Environment Runbook

This runbook deploys an isolated local test stack for `gpuf-s`, PostgreSQL,
Redis, Kafka/ZooKeeper, the management API, and `heartbeat_consumer`.

It does not reuse the default `GPUFabric` compose project, container names,
network, ports, or volumes. This keeps rollback simple and avoids touching an
already-running stack.

## Layout

- Compose file: `docker/gpuf_s_test_compose.yaml`
- Local env file: `docker/.env.gpuf-s-test`
- Env template: `docker/gpuf_s_test.env.example`
- DB schema: `scripts/db.sql`
- DB points migration: `scripts/device_points_daily_incremental.sql`
- Test seed: `scripts/gpuf_s_test_seed.sh`
- TLS files: `docker/cert.pem`, `docker/key.pem`, `docker/ca-cert.pem`
- Backup directory: `backups/gpuf-s-test/<timestamp>/`

Default local ports:

| Service | Host port |
|---|---:|
| gpuf-s control TLS | 17100 |
| gpuf-s proxy | 17101 |
| gpuf-s public | 18180 |
| api-server | 18181 |
| inference gateway | 18182 |
| PostgreSQL | 15432 |
| Redis | 16379 |
| ZooKeeper | 12181 |
| Kafka host listener | 39092 |

## Preflight

```bash
git status --short
docker compose version
docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
ss -ltnp
```

Confirm the default test ports above are free. If any port is occupied, edit the
matching variable in `docker/.env.gpuf-s-test`.

## Configure Secrets

```bash
cp docker/gpuf_s_test.env.example docker/.env.gpuf-s-test
```

Edit `docker/.env.gpuf-s-test` before shared use:

- `GPUF_TEST_POSTGRES_PASSWORD`: unique test DB password.
- `GPUF_TEST_GATEWAY_TOKEN`: exactly 48 characters, used as Bearer token for
  the inference gateway seed row.
- `GPUF_TEST_BANKING_API_TOKEN`: at least 32 characters, required by all
  provider pre-evaluation endpoints.
- `GPUF_TEST_BANKING_SERVICE_SUBJECT`: stable internal service subject used only
  for hashed idempotency scope; do not put a user ID or tenant ID here.
- `GPUF_TEST_BENCHMARK_PRODUCER_TOKEN`: separate token for the trusted
  benchmark registration endpoint; never reuse the banking API token.
- `GPUF_TEST_BENCHMARK_ED25519_PUBLIC_KEYS_JSON`: JSON object mapping approved
  benchmark key IDs to base64 Ed25519 public keys; private keys remain in runners.
- `GPUF_TEST_STORE_RAW_EVIDENCE`: keep `false` unless a reviewed retention need
  requires temporary raw offline evidence.
- `GPUF_TEST_RAW_EVIDENCE_TTL_DAYS`: raw evidence TTL from 1 to 90 days when
  retention is enabled.
- `GPUF_TEST_CLIENT_ID_HEX`: 32 hex characters, used by `gpuf-c --client-id`.

Do not commit `docker/.env.gpuf-s-test`. The committed compose uses a dummy
`--api-key` value for the legacy gpuf-s public listener; the gateway bearer token
is seeded into PostgreSQL instead of being placed in `gpuf-s` argv.

## TLS Certificates

For a local-only test stack, generate disposable certs:

```bash
cd docker
bash ../scripts/create_cert.sh
cd ..
```

Production or shared test hosts must replace these files with environment
specific certificates. Do not commit PEM files.

## Backup Before Deploy

Use a timestamped directory. This captures the current git revision, compose
configuration, env template, existing default DB if present, and existing test DB
if present.

```bash
TS="$(date +%Y%m%d-%H%M%S)"
BK="backups/gpuf-s-test/$TS"
mkdir -p "$BK"

git rev-parse HEAD > "$BK/git-revision.txt"
cp docker/gpuf_s_test_compose.yaml "$BK/"
cp docker/gpuf_s_test.env.example "$BK/"

docker exec gpuf-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/default-gpuf-postgres-before.sql" || true

docker exec gpuf-s-test-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/test-postgres-before.sql" || true
```

## Build Images

If local network proxy is needed during Docker build, use the host-reachable
proxy address rather than `127.0.0.1` inside the build container:

```bash
env -u http_proxy -u https_proxy -u all_proxy -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
  docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml build \
  --build-arg HTTP_PROXY=http://<host-reachable-proxy>:<proxy-port> \
  --build-arg HTTPS_PROXY=http://<host-reachable-proxy>:<proxy-port> \
  --build-arg http_proxy=http://<host-reachable-proxy>:<proxy-port> \
  --build-arg https_proxy=http://<host-reachable-proxy>:<proxy-port> \
  gpuf-s api-server heartbeat-consumer
```

If no proxy is needed:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml build gpuf-s api-server heartbeat-consumer
```

To package a binary already built and tested on the local host, reuse the existing
runtime image without compiling inside Docker:

```bash
cargo build --release -p gpuf-s --bin api_server
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp target/release/api_server "$STAGE/"
docker build -f docker/Dockerfile.prebuilt-runtime \
  --build-arg BASE_IMAGE=GPUFabric/api_server:latest \
  --build-arg BIN=api_server \
  -t GPUFabric/api_server:latest "$STAGE"
```

The local binary must be ABI-compatible with the runtime image. Run `ldd` and the
API health check after packaging. This path changes only the binary layer and does
not copy local environment files or credentials into the image.

## Deploy

Apply the additive pre-evaluation migration before starting the new API binary:

```bash
set -a
source docker/.env.gpuf-s-test
set +a

PGPASSWORD="$GPUF_TEST_POSTGRES_PASSWORD" psql \
  -h 127.0.0.1 -p "$GPUF_TEST_POSTGRES_PORT" \
  -U "$GPUF_TEST_POSTGRES_USER" -d "$GPUF_TEST_POSTGRES_DB" \
  -f scripts/prod_schema_add_pre_evaluation_reports.sql

PGPASSWORD="$GPUF_TEST_POSTGRES_PASSWORD" psql \
  -h 127.0.0.1 -p "$GPUF_TEST_POSTGRES_PORT" \
  -U "$GPUF_TEST_POSTGRES_USER" -d "$GPUF_TEST_POSTGRES_DB" \
  -f scripts/prod_schema_add_technical_asset_snapshots_v2.sql

PGPASSWORD="$GPUF_TEST_POSTGRES_PASSWORD" psql \
  -h 127.0.0.1 -p "$GPUF_TEST_POSTGRES_PORT" \
  -U "$GPUF_TEST_POSTGRES_USER" -d "$GPUF_TEST_POSTGRES_DB" \
  -f scripts/prod_schema_complete_pre_evaluation_v1.sql

PGPASSWORD="$GPUF_TEST_POSTGRES_PASSWORD" psql \
  -h 127.0.0.1 -p "$GPUF_TEST_POSTGRES_PORT" \
  -U "$GPUF_TEST_POSTGRES_USER" -d "$GPUF_TEST_POSTGRES_DB" \
  -f scripts/prod_schema_add_gpu_health_daily_stats.sql
```

The API runtime account no longer creates or seeds these tables during startup.
If an earlier experimental deployment contains JSONB snapshots or rows without
`report_sha256`, the migration fails instead of marking those rows trustworthy.
Export and review those legacy rows, then remove them through an approved test-data
cleanup before rerunning the migration.

Privacy defaults retain the offline evidence SHA-256 but not the original JSON.
Existing raw evidence receives a 30-day transition expiry when this migration is
applied, capped at 90 days from the original insert time. The API process sweeps
expired evidence hourly. Operators can also run the equivalent cleanup manually:

```sql
UPDATE pre_evaluation_report_evidence
SET evidence_json = NULL, retention_expires_at = NULL, purged_at = NOW()
WHERE evidence_json IS NOT NULL AND retention_expires_at <= NOW();
```

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml up -d
```

Check status:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml ps

docker logs --tail 100 gpuf-s-test
docker logs --tail 100 gpuf-s-test-api-server
docker logs --tail 100 gpuf-s-test-heartbeat-consumer
```

Expected `gpuf-s-test` log:

```text
gpuf-server listening on ports: Control=17000 (tls=true), Proxy=17001, Public=18080, API=18081, InferenceGateway=8081
Connected to database successfully
Connected to Redis successfully
Inference Gateway listening on port 8081
```

## Smoke Tests

API health:

```bash
curl -fsS http://127.0.0.1:18181/api/models/get
```

Kafka topics:

```bash
docker exec gpuf-s-test-kafka kafka-topics \
  --bootstrap-server kafka:29092 \
  --list
```

Expected topics include `client-heartbeats` and `request-message`.

Control TLS with local CA:

```bash
openssl s_client -connect 127.0.0.1:17100 \
  -servername localhost \
  -CAfile docker/ca-cert.pem \
  -verify_return_error \
  -verify_hostname localhost </dev/null
```

Run `gpuf-c` against the test stack:

```bash
target/debug/gpuf-c \
  --client-id "$GPUF_TEST_CLIENT_ID_HEX" \
  --server-addr 127.0.0.1 \
  --control-port 17100 \
  --proxy-port 17101 \
  --local-addr 127.0.0.1 \
  --local-port 11434 \
  --worker-type tcp \
  --engine-type ollama \
  --cert-chain-path docker/ca-cert.pem \
  --control-tls \
  --control-tls-server-name localhost
```

Ollama proxy request:

```bash
curl -sS -i http://127.0.0.1:18180/v1/chat/completions -H "Authorization: Bearer $GPUF_TEST_GATEWAY_TOKEN" -H "Content-Type: application/json" -d "{\"model\":\"llama3.2:latest\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply only: OK\"}],\"max_tokens\":8,\"temperature\":0,\"stream\":false}"
```

Port `18180` validates the token, selects an online client that advertises the
requested model, opens a temporary connection through the proxy port, and
forwards the HTTP request to the client's configured local service. Use this path
for `--engine-type ollama` clients.

Port `18182` is the command-based inference gateway. Its completion, chat, and
embedding tasks require a compatible `--engine-type llama` worker with a loaded
model; it does not proxy requests to a local Ollama server. Existing clients may
continue using either surface according to their configured engine type.

## Optional Online Client Benchmark

The online benchmark is disabled by default. Enable it only after setting the
same HMAC identity for both `gpuf-s` and `api-server`:

```dotenv
GPUF_TEST_ONLINE_BENCHMARK_ENABLED=true
GPUF_TEST_ONLINE_BENCHMARK_HMAC_SECRET=<shared-random-secret-at-least-32-bytes>
GPUF_TEST_ONLINE_BENCHMARK_KEY_ID=gpuf-online-test-2026-08
```

Deploy the server components before the v3 desktop client. Protocol v1/v2
clients never receive benchmark commands and continue to generate reports with
no benchmark evidence. A v3 client receives at most one bounded task after it
reports a local model. Missing models, task rejection, timeout, inference
failure, invalid results, or evidence persistence failure are logged and never
block heartbeat handling or report creation.

Successful results create insert-only evidence for `tokens_per_second` and
`sustained_throughput_percent`. The API revalidates the HMAC, stored payload,
source reference, task claims, and raw trial counters whenever a new report is
generated. Previously generated reports are immutable and are not rewritten.

Build locally before updating the test services:

```bash
cargo test -p common --lib
cargo test -p gpuf-c --lib benchmark::tests
cargo test -p gpuf-s --lib
cargo build --release -p gpuf-s --bin gpuf-s --bin api_server
cargo build --release -p gpuf-c --bin gpuf-c --features metal
```

## Backup After Deploy

```bash
TS="$(date +%Y%m%d-%H%M%S)"
BK="backups/gpuf-s-test/$TS"
mkdir -p "$BK"

git rev-parse HEAD > "$BK/git-revision.txt"
cp docker/gpuf_s_test_compose.yaml "$BK/"
cp docker/gpuf_s_test.env.example "$BK/"

docker exec gpuf-s-test-postgres pg_dump -U postgres -d GPUFabric \
  > "$BK/test-postgres-after.sql"

docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml config > "$BK/compose-rendered.yaml"

docker ps --filter 'name=gpuf-s-test' \
  --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}' > "$BK/container-status.txt"

docker exec gpuf-s-test-kafka kafka-topics \
  --bootstrap-server kafka:29092 \
  --list > "$BK/kafka-topics.txt"

chmod -R go-rwx "$BK"
```

## Rollback

For a failed test deployment, stop and remove only the test stack:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml down
```

To discard test data and start from a clean DB:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml down -v
```

To restore a previous test DB dump:

```bash
docker compose --env-file docker/.env.gpuf-s-test \
  -f docker/gpuf_s_test_compose.yaml up -d postgres

cat backups/gpuf-s-test/<timestamp>/<dump>.sql | \
  docker exec -i gpuf-s-test-postgres psql -U postgres -d GPUFabric
```

The default running stack uses separate names such as `gpuf-postgres`,
`gpuf-redis`, `gpuf-kafka`, and `frpx-network`. Do not run `down -v` against
`docker/gpuf_s_compose.yaml` during test rollback unless the explicit goal is to
destroy the default stack data.
