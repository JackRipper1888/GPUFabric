# GPUFabric Security Review & Hardening Plan

This document summarizes security risks and recommended mitigations for the **GPUFabric** repository based on the current architecture and code paths (primarily `gpuf-s` API server, heartbeat consumer/Kafka ingestion, and DB scripts).

## Scope and assumptions

- **In-scope**
  - `gpuf-s` API server (`/api/user/*` endpoints, routing, CORS)
  - Heartbeat ingestion pipeline (Kafka consumer + DB updates)
  - DB initialization scripts (`scripts/db.sql`) and stats/points materialization
- **Out-of-scope (not fully reviewed)**
  - All networking entry points in `gpuf-c` and other auxiliary components
  - Infrastructure configuration (K8s, systemd, Nginx, firewall rules)

Where something depends on infra (e.g., Kafka ACLs), this doc calls it out explicitly.

## Threat model summary

- **External attacker (internet)**
  - Calls public API endpoints directly
  - Exploits missing auth/authorization (IDOR)
  - Attempts DoS via high QPS / large payloads
- **Internal attacker / compromised client**
  - Publishes forged heartbeat messages to Kafka
  - Abuses timestamps to influence stats/points
- **Supply chain / dependency risk**
  - Vulnerable crates / future incompatibilities

## Findings (prioritized)

### P0 — Missing authentication/authorization → IDOR (horizontal privilege escalation)

**Risk**

Many endpoints accept `user_id` from query/body and use it to decide which user’s data to read/write. If the API is reachable without strict authentication, an attacker can enumerate/guess `user_id` and access or mutate other users’ data.

**Typical impact**

- Read another user’s client list, points, stats, monitor info
- Modify another user’s client info (e.g., via edit endpoints)

**Recommended fix (must-do)**

- Add an **authentication middleware** for `/api/user/*`.
- Do **not** trust `user_id` from request parameters.
  - Instead: derive `user_id` from the authenticated token (JWT `sub`, or a DB-backed token lookup).
- Add **authorization checks** on all operations that touch `client_id`:
  - Ensure the `client_id` belongs to the authenticated `user_id`.

**Implementation approach**

- Introduce an `AuthContext { user_id, client_id(optional), scopes }` extracted from token.
- Update handlers to accept `Extension<AuthContext>` and remove `user_id` from public API where possible.
- For backward compatibility, allow `user_id` parameter temporarily but **ignore it** and log a warning.

---

### P0 — Overly permissive CORS

**Risk**

Using permissive CORS expands the attack surface. If later you store auth tokens in browser-accessible storage or use cookies, this can enable cross-site abuse.

**Recommended fix**

- Replace permissive CORS with **allowlist** of trusted origins in production.
- Ensure preflight settings (`Allow-Headers`, `Allow-Methods`) are minimal.

---

### P0 — Heartbeat/Kafka forgery can poison online status & points/stats

**Risk**

The system’s monitoring/online status and stats depend on heartbeat messages. If Kafka is writable by untrusted producers or client identity is forgeable, attackers can:

- Mark assets online/offline incorrectly
- Inflate stats and downstream points (even if bucketed idempotency exists)

**Recommended fixes**

- **Kafka authentication & ACLs**
  - Enforce producer authentication (SASL/SCRAM or mTLS).
  - Restrict who can publish to heartbeat topic.
- **Message authentication**
  - Add message signatures (HMAC/Ed25519) bound to a per-client secret/public key.
  - Server verifies signature before processing.
- **Timestamp hardening**
  - Prefer broker append time (`LogAppendTime`) over producer-controlled create time.
  - Apply reasonable validation window (reject heartbeats too far in the future/past).

---

### P1 — Sensitive logging / data leakage

**Risk**

Logging raw payloads or detailed system metrics can leak sensitive operational information (client identifiers, hardware info, network throughput). If logs are shipped to shared systems, this increases exposure.

**Recommended fixes**

- Remove or gate “raw payload” logs behind debug flags; log only:
  - payload length
  - client_id (masked)
  - decoding result
- Apply masking to identifiers:
  - `client_id`: keep prefix/suffix, mask middle
- Ensure production defaults: `info` level; `debug` only for temporary diagnosis.

---

### P1 — DoS / resource exhaustion (API & consumer)

**API risks**

- High QPS unauthenticated access
- Large request bodies
- Expensive DB queries without timeouts

**Consumer risks**

- Very large Kafka messages or malformed payloads
- Lower throughput due to per-message transactions (more stable, but can backlog)

**Recommended fixes**

- API server
  - Add rate limiting per IP / per token
  - Add request timeout and DB timeout
  - Enforce body size limits
  - Constrain pagination parameters (already partly done for points)
- Heartbeat consumer
  - Enforce payload length upper bound before decode
  - Add limited concurrency (bounded worker pool) if needed
  - Add backpressure/monitoring for consumer lag

---

### P2 — DB scripts and operational safety

**Risk**

`scripts/db.sql` contains destructive operations (e.g., `DROP ... IF EXISTS`). In open source, users may run it directly in production, causing unintended data loss.

**Recommended fixes**

- Split into:
  - `schema.sql` (tables, indexes, functions)
  - `seed.sql` (test data; not for production)
  - migrations (incremental changes)
- Avoid destructive `DROP` in default scripts or require an explicit “I know what I’m doing” switch.

---

## Hardening roadmap (actionable)

### Phase 1 (P0) — Block the biggest real-world exploits

- **Auth middleware** for `/api/user/*`
- Replace **permissive CORS** with origin allowlist
- Kafka **ACLs** or message **signature verification**

### Phase 2 (P1) — Reduce abuse and leakage

- Rate limiting + request/body limits + timeouts
- Logging redaction / reduce debug logs
- Consumer payload length checks and monitoring

### Phase 3 (P2) — Operational maturity

- DB scripts migration-ize
- Add security scanning in CI (dependabot, `cargo audit`)

## Concrete technical方案（落地清单）

### 1) API 鉴权与授权（建议最先做）

- Add module: `gpuf-s/src/api_server/auth.rs`
  - Token extraction from `Authorization: Bearer <token>`
  - Validate token against DB `tokens` table (or JWT verification)
  - Produce `AuthContext { user_id, ... }`
- Add middleware in router creation
  - Apply to `/api/user/*` routes
- Update handlers
  - Remove reliance on user-provided `user_id`
  - Add checks that `client_id` belongs to `AuthContext.user_id`

### 2) CORS 生产配置

- Load allowed origins from env (`ALLOWED_ORIGINS`)
- Configure `CorsLayer` with allowlist rather than permissive

### 3) Heartbeat topic protection

- Infra
  - Enable Kafka auth (SASL/mTLS)
  - Configure topic ACLs
- App
  - Add signature to heartbeat message
  - Validate signature before DB writes
  - Enforce timestamp sanity window

### 4) DoS 防护

- Add rate limit layer (tower)
- Add request body limit
- Add timeouts
- Add pagination upper bounds universally

### 5) 日志脱敏

- Mask `client_id`/IP
- Remove raw payload dumps

## Verification checklist

- API
  - Unauthenticated requests to `/api/user/*` must return 401
  - Authenticated user A cannot access user B’s clients/points
- Kafka/consumer
  - Untrusted producer cannot publish to heartbeat topic
  - Invalid signatures are rejected
  - Heartbeats with extreme timestamps are rejected
- Observability
  - Logs do not contain raw payloads or full identifiers

