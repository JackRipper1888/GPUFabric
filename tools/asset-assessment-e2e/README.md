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
