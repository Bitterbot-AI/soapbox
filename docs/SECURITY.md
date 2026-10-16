# Security and operating limits

## Trust model for v0.1

Use explicit, trusted operators and nonsensitive pilot inputs. The coordinator sees input
and result data. A worker sees its assigned plaintext. The SQLite database is not encrypted
by Soapbox; restrict directory permissions and use encrypted disks where appropriate.
Bearer tokens authorize provisioned roles and memberships, not hardware attestation.

- Default bind: 127.0.0.1. Remote workers require an HTTPS coordinator URL.
- A remote coordinator bind requires --allow-remote and operator-managed TLS/network controls.
  Do not expose its plain HTTP listener directly to the internet.
- No browser CORS access; Origin headers are rejected. Desktop requests pass through the gateway.
- The client never follows redirects with its credential. Request/response sizes are bounded.
- No shell commands, uploads of executable code, buyer-supplied model URLs, or arbitrary mounts.
- Workers only call an already-configured local Ollama backend for the allowed model.
- Admin pause drains new allocations; it is not an emergency process kill.
- Cancellation revokes acceptance. Physical GPU cancellation can lag or be unsupported.
- Account balances and reservations commit with job transitions in SQLite.
- Lease tokens are unique per attempt; expired or cancelled attempts cannot earn credits.
- Current receipts are unsigned coordinator records; embedding acceptance checks shape only.

## What is not provided

Public marketplace security, confidential computing, malicious-host resistance, Sybil
resistance, Byzantine scheduling, independent GPU usage attestation, OS-enforced memory/
GPU limits, automatic idle detection, malicious model defense, mTLS, credential rotation
without restart, HA coordinator failover, complete rate limiting, durable distributed
storage, secure data erasure, or real payment settlement.

A loaded Ollama model may consume resources outside the worker's declared allowance.
Use dedicated/trusted hardware for this adapter until proper runtime isolation exists.
The worker observes model GPU residency; a model can be evicted or partly CPU-resident.
Do not bill hardware-hour guarantees from this signal.

Artifacts expire logically, but SQLite pages/WAL/backups may retain bytes. Job inputs,
results and receipts are retained for the pilot; define a retention policy before sensitive
or long-running deployments. Disk quotas and archival are needed beyond the bounded active queue.

## Before public operation

Complete Phase 2 and obtain an independent security review. Cover untrusted buyers and
workers, secret injection, egress abuse, resource exhaustion, dependency/image provenance,
revocation, anti-replay, measurement fraud, settlement recovery and pool isolation.
Test actual operating systems and GPU models. Passing the local suite is not that review.

Report vulnerabilities privately to the repository owner. No external disclosure endpoint
has been configured by this initial implementation.
