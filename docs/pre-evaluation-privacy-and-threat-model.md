# Pre-Evaluation Privacy And Threat Model

Visual reference: [architecture HTML](asset-assessment-architecture.html) and
[standalone SVG](svg/asset-assessment-architecture.svg).
Development baseline: [data model and roadmap](pre-evaluation-data-model-and-roadmap.md).
Cross-service contract:
[integration and task breakdown](pre-evaluation-cross-service-integration.md).

## Scope

This document covers the provider pre-evaluation APIs, existing `gpuf-c` telemetry,
offline `hw-asset-collector` evidence, report persistence, and server-side GPU model
enrichment. It does not define bank underwriting, legal ownership verification,
market pricing, or a trusted benchmark execution service.

## Trust Boundaries

| Input | Trust level | Meaning |
|---|---|---|
| Existing `gpuf-c` telemetry | Authenticated client telemetry | The server knows which enrolled client sent the data; the client can still misreport hardware or runtime values. |
| Offline collector document | Self-reported, challenge-bound | SHA-256 plus a single-use challenge detects modification and replay after collection; it does not prove the physical machine identity. |
| Server GPU specification table | Reviewed reference data | Adds theoretical vendor/model specifications; it is not a measurement of the submitted device. |
| Ownership, benchmark, pricing, and credit data | External trusted systems | These require separate evidence stores, policy versions, reviewers, and audit trails. Caller-supplied values are currently rejected. |

## Data Placement

Device-side components may collect hardware inventory and local runtime measurements.
They should not decide ownership status, market value, pledge rate, loan amount, report
status, or bank eligibility.

`api_server` owns normalization, privacy filtering, technical snapshot and pre-evaluation
draft generation, integrity hashes, retention enforcement, and access control. The draft
may maximize use of available hardware and telemetry data, but it must leave unsupported
business fields empty. A future benchmark runner should be a separate, allowlisted service.
It must not accept arbitrary shell commands from report requests.

Private business systems own identity documents, invoices, ownership evidence, pricing
policies, reviewer decisions, and formal credit results. Those records should be linked
by controlled references rather than copied into open-source client telemetry.

## Data Classification

| Class | Examples | Default handling |
|---|---|---|
| Public reference | GPU model specifications, schema versions | May be committed after source review. |
| Operational | Utilization, temperature, uptime, benchmark output | Authenticate, minimize, and retain only for a documented purpose. |
| Identifying | Client id, host name, IP address, account id | Keep internal; use explicit report labels or pseudonymous references. |
| Highly sensitive | Serial numbers, UUIDs, WWNs, asset tags, credentials, ownership documents | Do not collect in this report path; reject or store in a separate controlled system. |

## Main Threats And Controls

| Threat | Current control | Residual risk |
|---|---|---|
| Forged hardware telemetry | Enrolled client lookup, strict parsing, server-side spec enrichment | A compromised client can still lie; this is not remote attestation. |
| Offline evidence tampering | Exact-document SHA-256 and single-use five-minute challenge | The collector or host can fabricate data before hashing. |
| Replay | Redis challenge consumption with `GETDEL` | Redis availability and configuration remain part of the trust boundary. |
| Management token theft | Minimum length, constant-time comparison, multi-token rotation | A shared token has broad scope; use a gateway, TLS, short rotation windows, and audit logs. |
| Raw evidence disclosure | Default hash-only storage, optional 1-90 day TTL, hourly purge, manual purge API | Backups and replicas must follow the same retention policy. |
| Report or database tampering | Immutable report rows and report SHA-256 verification on read | A database administrator can replace both content and hash; external signing is not implemented. |
| Sensitive identifiers in reports | Reject serial-class fields; avoid default host/client names; hash online source references | Explicit operator-provided asset names may still contain identifying text. |
| Untrusted business results | Caller supplements are rejected | Trusted benchmark, ownership, pricing, and credit integrations are still required. |
| Benchmark command injection | No benchmark execution in the report API | A future runner must use fixed suites, fixed arguments, resource limits, and signed result envelopes. |

## Retention And Deletion

The immutable report snapshot contains normalized results and warnings, not raw offline
evidence. The evidence table always retains the exact uploaded document SHA-256. Raw JSON
is stored only when `GPUF_PRE_EVALUATION_STORE_RAW_EVIDENCE=true`, with
`GPUF_PRE_EVALUATION_RAW_EVIDENCE_TTL_DAYS` limited to 1-90 days.

`api_server` sweeps expired raw JSON hourly and the management API can purge a report's
raw evidence immediately. Database backups, replicas, log exports, and object storage are
separate copies and require matching lifecycle rules.

## Validation Expectations

- Verify unauthenticated provider requests return `401`.
- Verify invalid or placeholder server token configuration returns `503`.
- Verify replayed or modified offline evidence returns `422`.
- Verify serial-class identifiers return `422`.
- Verify default offline inserts retain a hash with `evidence_json IS NULL`.
- Verify opt-in raw evidence has an expiry no later than 90 days after insertion.
- Verify manual and hourly purge keep the evidence hash and report snapshot intact.
- Verify existing `gpuf-c/common` protocol messages remain unchanged.

## Non-Goals

The current implementation does not provide TPM/TEE attestation, vendor-signed device
identity, trusted ownership verification, certified benchmark execution, market pricing,
formal valuation, or a bank credit decision. Reports must continue to state these limits.
