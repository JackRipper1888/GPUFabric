# Asset Assessment Local Full-Chain E2E

This test-only stack proves the asset-assessment path with real local processes
and HTTP boundaries. It does not start or modify `new-api`; staging updates keep
`new-api` as the final deployment step.

## Coverage

- GPUFabric immutable technical report, snapshot, tenant isolation and T2 data
- PostgreSQL migrations and durable assessment state
- TLS MinIO private buckets with test-only SSE-S3 KMS configuration
- ClamAV scan, deterministic OCR and evidence review
- automatic and manual market observations, verification and immutable snapshot
- independently approved pricing policy, valuation and two-person review
- Chromium PDF rendering, SoftHSM2 signing and independent certificate checks
- report download, revocation and expiry enforcement
- signed new-api-compatible callback delivery through the durable outbox
- EICAR, digest mismatch, replay conflict, reviewer separation and callback
  replay/conflict/tamper negative gates

The local HSM, CA, market records, identities and KMS key are fixtures. They
prove integration behavior but are not production trust or licensed market
evidence.

## Run

Prerequisites:

- Docker Compose, Go 1.22 or later, `curl` and `sha256sum`
- sibling checkout `../asset-assessment-service`, or set
  `ASSESSMENT_SERVICE_DIR` to its absolute path
- `docker/.env.gpuf-s-test` with a valid `GPUF_TEST_BANKING_API_TOKEN`
- locally available pinned container images, or network access to fetch them

From the GPUFabric repository:

```bash
tools/asset-assessment-e2e/scripts/local-e2e.sh run
```

The script starts the existing isolated GPUFabric test Compose project, builds
the exact local `asset-assessment-service` checkout as a static binary, packages
it in the pinned runtime image, starts the assessment dependencies and executes
the full flow. Results are written under
`/tmp/gpuf-asset-assessment-e2e-results/<run-id>/` with restrictive permissions.

Other commands:

```bash
tools/asset-assessment-e2e/scripts/local-e2e.sh up
tools/asset-assessment-e2e/scripts/local-e2e.sh down
tools/asset-assessment-e2e/scripts/local-e2e.sh reset
```

`down` preserves assessment test volumes. `reset` removes only volumes owned by
the `gpuf-asset-assessment-local-e2e` project. Neither command stops the
GPUFabric test project or any new-api deployment.

## Local Endpoints

| Service | URL |
|---|---|
| GPUFabric API | `http://127.0.0.1:18181` |
| asset-assessment-service | `http://127.0.0.1:28092` |
| support callbacks/renderer/HSM | `http://127.0.0.1:28180` |
| MinIO S3 API | `https://127.0.0.1:29000` |

Loopback HTTP is deliberately used for local callbacks and service simulators.
A shared test or production deployment must use TLS or mTLS, managed secrets,
restricted service identities, cloud KMS or institutional HSM credentials, a
trusted certificate chain, licensed market feeds and approved reviewer identities.

## Verification

Run module tests independently with:

```bash
cd tools/asset-assessment-e2e
go test ./...
```

The runner exits non-zero at the first failed gate. A successful run prints the
artifact directory. Its manifest, PDF, detached signature and exported local CA
allow an independent verifier to recompute hashes and validate the test chain.

## Shared Test Environment

The runner can execute inside the shared assessment Docker network without
embedding deployment credentials. It reads the support token from
`E2E_SUPPORT_TOKEN` or `ASSESSMENT_PDF_RENDERER_TOKEN`, and reads assessment
subject/token pairs from `E2E_ASSESSMENT_CREDENTIALS_JSON` or the service's
existing `ASSESSMENT_SERVICE_CREDENTIALS_JSON`. Both the single `token` and
rotating `tokens` credential forms are supported. The legacy
`ASSESSMENT_SERVICE_TOKEN` and `ASSESSMENT_LEGACY_SERVICE_SUBJECT` pair can
supply the `new-api` client role.

Set these non-secret controls explicitly for a shared run:

```bash
E2E_ASSESSMENT_URL=http://asset-assessment-service:8092
E2E_SUPPORT_URL=https://assessment-report-support
E2E_GPUFABRIC_URL=http://gpuf-api-server:18081
E2E_ALLOW_CONTAINER_HTTP=true
E2E_CALLBACK_MODE=external
E2E_TENANT_REF=tenant-shared-test
```

Plain HTTP remains limited to loopback by default. The opt-in permits only
single-label container hostnames and private IP addresses; public HTTP targets
are still rejected. HTTPS targets do not require this opt-in.

The default `E2E_REPORT_LIFECYCLE_MODE=full` keeps the local revocation, short
expiry and callback-sink gates. A shared deployment with a long report validity
or no revocation test identity can explicitly use
`E2E_REPORT_LIFECYCLE_MODE=skip` together with
`E2E_CALLBACK_MODE=external`. That mode still exercises technical report
creation, T2 verification, evidence upload and integrity failures, malware
scanning, OCR, evidence review, market data, pricing approval, valuation,
two-person formal review, PDF rendering, HSM signing, private storage and
download verification. Verify external callback outbox delivery separately.

Build a portable runner for an Alpine assessment runtime with:

```bash
CGO_ENABLED=0 go build -trimpath -o asset-assessment-e2e-runner ./cmd/runner
```

Do not pass tokens on the command line or write them into runner logs. Inject
them through a restricted environment file or the platform secret manager.

### Continue an assessment created by new-api

The shared runner can complete an assessment that new-api already created,
without creating a duplicate technical report or assessment:

```bash
E2E_EXISTING_ASSESSMENT_ID=ASMT-2026-... \
E2E_TENANT_REF=tenant:hmac:v1:... \
E2E_CALLBACK_MODE=external \
E2E_REPORT_LIFECYCLE_MODE=skip \
asset-assessment-e2e-runner
```

This mode requires explicit shared service credentials and accepts only a
bounded `ASMT-` identifier. It requires the existing task to be a technically
verified T2 assessment at `evidence_pending`; otherwise it exits before creating
evidence. It then exercises the three evidence types, configuration-bound test
market data, independent policy approval, valuation, reviewer separation,
ordered two-person approval, report freeze, test-HSM signing and private PDF
download. The normal external outbox updates the matching new-api task.

Use this mode only in a controlled test environment. The generated evidence,
market observations, reviewer identities and signing chain are test fixtures.

### Shared Acceptance Record (2026-08-05)

Run `20260805T050741-33e85650` completed the shared-environment report mainline
for offline Apple M1 Pro asset `e5dd57907588424abb886eff4bcfd378`:

- assessment `ASMT-20260805-dbe8539c21d5ca3c`
- technical report `PRE-2026-08-3D53DBE960FE4D1E9A901F55B674B087`
- issued report `AER-20260805-a70dfc0d5454d12e`
- PDF SHA-256 `08134a637616e4395801392a8114e57323ea37e5c291ef1d8b3878bd563dd16a`
- ECDSA-P256-SHA256 signature and certificate-chain verification passed

The run covered T2 verification, tenant isolation, upload integrity failures,
EICAR rejection, OCR, evidence review, market insufficiency and success gates,
pricing dual approval, valuation, formal dual review, PDF rendering, HSM signing,
private storage and download verification. Its benchmark rows are explicitly
tagged `shared-test-e2e/api-regression-no-performance-claim`; they are regression
fixtures, not device performance claims.

Revocation and expiry were not executed because the shared deployment has no
`report-revoke-worker` test identity and uses 180-day report validity. The
external new-api callback returned `409 CALLBACK_BINDING_CONFLICT` because this
direct assessment run had no matching local new-api task. A callback acceptance
run must first create the task through new-api; do not report this callback as
passed until that prerequisite is satisfied.
