# Asset Assessment ssh test Release Plan

## 1. Scope and Frozen Revisions

Release target: the host addressed by `ssh test`.
Unless a command is explicitly marked as a local build step, all commands in
this document run after connecting to `ssh test`.

| Component | Current remote state | Target | Action |
|---|---|---|---|
| GPUFabric `api_server` | banking device route returns 404; mounted binary SHA-256 `9ccbe91f...` | clean build from `f8bc665` | migrate, canary, replace |
| asset-assessment-service | not deployed | `ea0f1be` | new isolated deployment |
| new-api | `new-api:micro-new-api`, two API containers and one worker | unchanged | **NO CHANGE** |

The new-api image, containers, environment file, Compose file and database
migrations are outside this release. The existing image has banking routes, but
its environment has no `BANKING_*`, `GPUFABRIC_*` or `ASSESSMENT_*` keys.
Therefore this release validates GPUFabric and the assessment service directly;
new-api callbacks and the browser-to-assessment path remain disabled.

At execution time, record immutable full commit IDs, binary/image SHA-256,
migration SHA-256 and the remote image/container IDs in a release manifest.
Never deploy from a dirty checkout.

## 2. Current Remote Constraints

- Root filesystem: 99 GiB total, 13 GiB free, 87% used.
- Memory: 3.5 GiB total, about 1.0 GiB available, no swap.
- Existing new-api traffic is balanced across `new-api-api-1` and
  `new-api-api-2`; neither container may be restarted by this release.
- GPUFabric currently binds host port `18081`.
- PostgreSQL `aliyun_test` already contains new-api banking tables but does
  not contain GPUFabric `pre_evaluation_reports` or
  `technical_asset_snapshots`.

No-go thresholds:

- less than 8 GiB free disk after backups;
- less than 768 MiB available memory for the core deployment;
- any unverified database backup;
- any target artifact without a recorded SHA-256;
- any new-api container/config/image change in the proposed command diff.

The full ClamAV/Chromium/MinIO/SoftHSM test fixture is not made persistent on
this low-memory host in the core release. Its complete chain has already passed
locally. Running the same fixture remotely is a separate on-demand action and
requires either at least 1.5 GiB free memory or approved swap provisioning.

## 3. Release Layout

Use a single release identifier:

```bash
RELEASE_ID="$(date -u +%Y%m%dT%H%M%SZ)-f8bc665-ea0f1be"
BACKUP_ROOT="/var/backups/gpunexus-asset-assessment/$RELEASE_ID"
INSTALL_ROOT="/opt/gpunexus/asset-assessment"
install -d -m 0700 "$BACKUP_ROOT"
install -d -m 0750 "$INSTALL_ROOT/releases/$RELEASE_ID"
```

Expected backup content:

```text
manifest.txt
sha256sums.txt
database/
  postgres-globals.sql
  aliyun_test.dump
  aliyun_test.schema.sql
services/
  gpuf-api-server.inspect.json
  gpuf-api-server.image.txt
  api_server.local-current
  api_server.local-current.sha256
  new-api-api-1.inspect.json
  new-api-api-2.inspect.json
  new-api-worker.inspect.json
  new-api-current-env.runtime
  new-api-compose.yml
artifacts/
  api_server.target.sha256
  asset-assessment-service.image.sha256
```

The new-api files are copied only as rollback evidence. They are not edited or
restored during this release.

## 4. Database Backup

### 4.1 Shared aliyun_test database

Before migrations, capture globals, a custom-format logical backup and a
schema-only snapshot. Run through the existing PostgreSQL container so no
credential is printed in terminal history:

```bash
docker exec postgres-test-env pg_dumpall -U postgres_test --globals-only \
  > "$BACKUP_ROOT/database/postgres-globals.sql"
docker exec postgres-test-env pg_dump -U postgres_test -d aliyun_test \
  --format=custom --compress=6 --no-owner --no-acl \
  > "$BACKUP_ROOT/database/aliyun_test.dump"
docker exec postgres-test-env pg_dump -U postgres_test -d aliyun_test \
  --schema-only --no-owner --no-acl \
  > "$BACKUP_ROOT/database/aliyun_test.schema.sql"
```

Verification is mandatory:

```bash
test -s "$BACKUP_ROOT/database/postgres-globals.sql"
test -s "$BACKUP_ROOT/database/aliyun_test.dump"
test -s "$BACKUP_ROOT/database/aliyun_test.schema.sql"
pg_restore --list "$BACKUP_ROOT/database/aliyun_test.dump" >/dev/null
sha256sum "$BACKUP_ROOT"/database/* > "$BACKUP_ROOT/database/SHA256SUMS"
sha256sum --check "$BACKUP_ROOT/database/SHA256SUMS"
```

Copy the backup directory off-host and verify the same checksum file before the
first migration. A backup that exists only on `ssh test` does not satisfy the
gate because the filesystem is already 87% used.

### 4.2 New assessment database

asset-assessment-service reuses the existing `postgres-test-env` instance on
`ssh test`, but uses a dedicated `asset_assessment` database and an
`asset_assessment` role. It does not write its tables into `aliyun_test`.
On first deployment there is no pre-release assessment data to back up. For
every later release:

```bash
docker exec postgres-test-env pg_dump -U postgres_test -d asset_assessment \
  --format=custom --compress=6 --no-owner --no-acl \
  > "$BACKUP_ROOT/database/asset_assessment.dump"
pg_restore --list "$BACKUP_ROOT/database/asset_assessment.dump" >/dev/null
```

## 5. Service and Configuration Backup

```bash
docker inspect gpuf-api-server \
  > "$BACKUP_ROOT/services/gpuf-api-server.inspect.json"
docker inspect new-api-api-1 \
  > "$BACKUP_ROOT/services/new-api-api-1.inspect.json"
docker inspect new-api-api-2 \
  > "$BACKUP_ROOT/services/new-api-api-2.inspect.json"
docker inspect new-api-worker \
  > "$BACKUP_ROOT/services/new-api-worker.inspect.json"

cp -a /home/api_server/api_server.local-current \
  "$BACKUP_ROOT/services/api_server.local-current"
cp -a /root/work/docker_images/docker-compose.yml \
  "$BACKUP_ROOT/services/new-api-compose.yml"
cp -a /root/work/docker_images/new-api-current-env.runtime \
  "$BACKUP_ROOT/services/new-api-current-env.runtime"
chmod 0600 "$BACKUP_ROOT/services/new-api-current-env.runtime"
```

Record, but do not prune, the old GPUFabric image ID. Keep the previous binary
at `/home/api_server/api_server.rollback-$RELEASE_ID`. Preserve the existing
new-api image ID `sha256:5764fd7f...` and all three container IDs unchanged.

Generate `sha256sums.txt` only after all copies are complete, then copy the
whole backup off-host. The manifest records hostname, UTC time, operator,
current container IDs, target commits and all checksums.

## 6. GPUFabric Database Migration

Apply these committed PostgreSQL migrations in order:

| Order | File | SHA-256 |
|---:|---|---|
| 1 | `prod_schema_add_pre_evaluation_reports.sql` | `e5a70c01f1419861f9d5e403fda3df7767f88466c1c13ed8ab26523f430c8c49` |
| 2 | `prod_schema_add_technical_asset_snapshots_v2.sql` | `c32c0a332ff862c25f9c88c8e3c812ec8f203325bc15593d468fd07a60d58cde` |
| 3 | `prod_schema_complete_pre_evaluation_v1.sql` | `c7e3940f0836fc10f28035ed134040b9bf9da1ff2ae1a68d580f44d6bcc790b6` |
| 4 | `prod_schema_add_gpu_health_daily_stats.sql` | `ab471cd24510e8f12de315a71816d0febba09a41cb777f7ad30eaa2cea1a6f41` |

Each file has its own transaction. Use `ON_ERROR_STOP` and stop the release
immediately on any non-zero exit:

```bash
docker exec -i postgres-test-env psql \
  -U postgres_test -d aliyun_test -v ON_ERROR_STOP=1 < migration.sql
```

Post-migration checks:

- all expected tables, constraints, indexes and immutable triggers exist;
- no legacy row has a missing report/snapshot SHA-256;
- a transaction attempting to mutate an immutable snapshot is rejected;
- existing new-api banking row counts are unchanged;
- a second migration run is idempotent.

These are additive migrations. A normal application rollback leaves the new
tables in place. Do not execute the technical snapshot rollback SQL during a
binary rollback because it drops stored data.

## 7. GPUFabric Service Deployment

Build `api_server` locally from a clean archive of `f8bc665`, run
`cargo test -p gpuf-s --lib`, and record the binary SHA-256. Transfer it as a
candidate file; never compile on the 3.5 GiB test host.

Create a mode-`0600` runtime environment containing:

- the existing database URL, captured without printing it;
- a new random `GPUF_BANKING_API_TOKEN`;
- stable `GPUF_BANKING_SERVICE_SUBJECT=asset-assessment-service`;
- a separate benchmark producer token;
- the approved test benchmark Ed25519 public keyring;
- raw evidence retention disabled by default.

Keep the corresponding assessment client token in the assessment secret file.
Do not put either token in Compose YAML, shell history or the release manifest.

Canary:

1. Start the candidate on loopback port `18083`, attached to the dedicated
   assessment network and using the same database.
2. Require `/api/models/get` success.
3. Require the banking route to return authentication failure with no/wrong
   token and success with the correct token.
4. Create and read a disposable pre-evaluation, verify its hash, then remove
   only non-immutable disposable idempotency state if cleanup is required.
5. Stop the canary before switching port `18081`.

Switch:

1. Save the current binary as
   `api_server.rollback-$RELEASE_ID`.
2. Stop only `gpuf-api-server`.
3. Install the candidate atomically at the mounted path.
4. Recreate/start `gpuf-api-server` with the reviewed environment and network.
5. Require health and route tests within 60 seconds.

Rollback trigger: startup failure, two consecutive health failures, banking
auth bypass, schema/hash mismatch, or any unexpected database write.

GPUFabric rollback:

```bash
docker stop gpuf-api-server
install -m 0755 \
  "/home/api_server/api_server.rollback-$RELEASE_ID" \
  /home/api_server/api_server.local-current
docker start gpuf-api-server
```

If the container definition changed, recreate it from the saved inspect/config
record using the old binary. Leave additive tables in place.

## 8. asset-assessment-service Deployment

Build the image locally from clean commit `ea0f1be`. The runtime image contains
only the static binary and the 15 checksum-pinned SQL migrations. Record both
the binary and image digest before transfer.

Deploy an isolated Compose project with:

- the existing remote `postgres-test-env` PostgreSQL instance;
- dedicated `asset_assessment` database and `asset_assessment` role;
- asset service attached to its private network and
  `micro-new-api_default` with alias `asset-assessment-service`;
- host exposure limited to `127.0.0.1:8092`;
- GPUFabric URL resolved on the internal assessment network;
- callback URL empty, because new-api is explicitly unchanged;
- unique subject-bound service credentials;
- HMAC evidence gateway only as an integration placeholder;
- formal report/lifecycle flags disabled on the persistent core service.

The local full-chain fixture remains the evidence for ClamAV, private S3/SSE,
OCR, market governance, valuation, Chromium PDF, SoftHSM2 signing, callback
replay controls, revocation and expiry. Do not label the persistent remote
core service as production-ready while renderer, HSM, private report storage
and licensed market data are absent.

Migration gate:

1. Verify `postgres-test-env` with `pg_isready`.
2. Create the dedicated role/database using a mode-`0600` generated password
   file; do not print the password or store it in Compose YAML.
3. Attach `postgres-test-env` to the private assessment Docker network with
   a stable network alias; do not restart the PostgreSQL container.
4. Run one assessment container with migration-on-start and no public port.
5. Verify `assessment_schema_migrations` has exactly 15 rows and each stored
   checksum matches the image files.
6. Start the persistent API and require `/readyz`.

Direct smoke tests:

- wrong/missing service token is rejected;
- tenant isolation returns 404 without leaking identifiers;
- a valid T1/T2 request reaches the expected technical result;
- the outbox persists events but does not attempt new-api delivery;
- restart preserves the assessment and audit trail.

New-service rollback:

```bash
docker compose -p asset-assessment-test down
```

This stops the assessment service without stopping or deleting
`postgres-test-env`. Never drop the `asset_assessment` database as an
application rollback. If a prior assessment image exists in later releases,
retag and restart that digest. Restore `asset_assessment.dump` only after
stopping every assessment writer and taking a failed-state dump.

## 9. Database Restore Policy

### Normal rollback

- roll back binaries/images and configuration;
- keep additive GPUFabric tables;
- keep the assessment database/volume;
- do not restore the shared `aliyun_test` database.

### Data corruption rollback

This is a separate incident operation:

1. stop GPUFabric API and every writer to `aliyun_test`, including all
   new-api API/worker containers;
2. take a final failed-state custom dump;
3. record the exact recovery point and obtain explicit approval, because this
   interrupts unrelated test services;
4. restore into a new database name first;
5. compare schema, critical row counts and application smoke tests;
6. switch clients only after validation.

An in-place full restore is the last resort. Target RPO is the start of the
release window; target RTO is 15 minutes for service rollback and 60 minutes
for coordinated shared-database recovery.

## 10. Acceptance and Automatic Stop Conditions

Acceptance:

- remote GPUFabric banking route is present and enforces its token;
- remote asset service is healthy after a restart;
- database backups exist both on-host and off-host and pass checksums;
- GPUFabric immutable report/snapshot checks pass;
- assessment migrations and audit/outbox persistence pass;
- no new-api container ID, image ID, start time, Compose checksum or
  environment-file checksum changes;
- no new public listener other than the existing GPUFabric port; assessment
  binds loopback only;
- release evidence is archived with mode `0700`.

Stop and roll back the active service step on:

- migration error or checksum mismatch;
- health not restored in 60 seconds;
- authentication/tenant isolation regression;
- unexpected new-api restart or configuration checksum change;
- OOM event, available memory below 256 MiB, or disk below 5 GiB;
- callback traffic observed despite callback being disabled;
- any data invariant or immutable-trigger failure.

## 11. Evidence and Change Record

Archive:

- pre/post container lists and inspect records;
- pre/post database schema and critical row counts;
- target commit IDs and artifact/migration SHA-256;
- test commands and exit status;
- health/API response status with tokens and private fields redacted;
- deployment and rollback timestamps;
- confirmation that new-api was unchanged.

The release is complete only when the off-host backup checksum, direct service
tests and unchanged-new-api proof all pass. new-api/frontend integration remains
a separate release requiring explicit authorization to add its upstream and
callback environment.
