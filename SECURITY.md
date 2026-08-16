# Security Policy

GPUFabric is open source, but deployment credentials, device telemetry, reports,
database contents, logs, and customer data are not public project data.

## Reporting A Vulnerability

Do not open a public issue for suspected vulnerabilities, leaked credentials, or
private deployment data. Use the repository's private
[GitHub security advisory form](https://github.com/nexus-gpu/GPUFabric/security/advisories/new).

Include the affected revision, component, reproduction steps, impact, and any
suggested mitigation. Remove live tokens, personal data, device serials, and customer
identifiers from the report unless they are essential to reproduce the issue.

## Supported Versions

Security fixes target the latest release and the current default branch. Older
releases may require upgrading before a fix can be applied.

## Deployment Baseline

- Keep management APIs on loopback or behind an authenticated reverse proxy and firewall.
- Use TLS for traffic that crosses a host or trusted private network boundary.
- Store database passwords and API tokens in a secret manager or ignored environment file.
- Rotate provider API credentials with `GPUF_BANKING_API_TOKENS`; remove the old token after rollout.
- Do not embed service credentials in browser or mobile application code.
- Restrict PostgreSQL, Redis, and Kafka to trusted networks with independent authentication.
- Review logs, backups, crash dumps, and exported reports as sensitive operational data.

## Privacy Defaults

Pre-evaluation reports minimize retained evidence:

- Offline raw evidence is not stored unless explicitly enabled.
- The evidence SHA-256 is retained for integrity and audit correlation.
- Optional raw retention is limited to 1-90 days and swept hourly by `api_server`.
- Raw evidence can be purged without deleting the immutable report snapshot or hash.
- Serial numbers, UUIDs, WWNs, and asset tags are rejected by the offline report API.
- Client IDs and collector host names are not exposed as default report labels.

These controls reduce disclosure risk; they do not turn self-reported telemetry into
hardware attestation. See the
[pre-evaluation privacy and threat model](docs/pre-evaluation-privacy-and-threat-model.md).

## Repository Hygiene

Before publishing changes, run the configured security release checks and inspect the
diff for credentials, private endpoints, database dumps, generated reports, and local
environment files. The repository ignores common secret and backup paths, but ignore
rules are not a substitute for review.
