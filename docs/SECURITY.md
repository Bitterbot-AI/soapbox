# Security and operating limits

> Current isolation and USDC extension: [Phase 2 isolation and payments](PHASE-2-ISOLATION-PAYMENTS.md). Earlier TEST_CREDIT/shared-host descriptions below refer to the original pilot.

## Trust model for v0.2

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

## RPC credentials and error responses

### Mesh permission changes (authorization-recovery update)

Scope edits and full revocation cancel affected queued/running jobs in the same
SQLite transaction as the admission change. Retained scopes keep their jobs.
HTTP submission revalidates the credential and current scope inside the enqueue
transaction after request-body extraction. Worker claim, renewal and result
commit recheck the current owner admission. The sweeper also cancels orphaned
mesh work left by older interrupted revocations. Successfully committed results
remain available for idempotent completion replay; revocation does not undo work.

The matching Bitterbot gateway persists disabled grants and reconciles them on
startup and service ticks. Pending revocation remains visible until coordinator
acknowledgement; local gateway denial is not proof of worker shutdown while the
coordinator is unavailable. Requests serialized after a successful revocation
commit cannot enqueue or claim work using the revoked mesh admission. Physical
GPU cancellation is still bounded by worker/runtime behavior, not this API check.

This applies to dynamic mesh admissions. Static coordinator principals keep
their existing provisioning model. Paid mode remains unavailable to mesh peers;
the Base Sepolia payment and dedicated-wallet policies are unchanged. This is an
rc7 runtime update; upgrade the gateway and runtime together. Older archives do not contain these fixes.

### Cancellation fences (rc8)

The admin-only `POST /v1/admissions/{publicKeyHex}/cancel` endpoint records an
owner-bound mesh idempotency key and cancels matching free work in one transaction.
It returns only the key, optional job ID and terminal status. The matching gateway
binds the public key to the authenticated P2P peer, including after revocation.
No grant or submission is needed. Late enqueue is rejected after restart or
re-admission; already completed effects are not undone. Missing-job fences are
limited to 4,096 per owner and 65,536 overall; existing jobs and repeated cancels
remain cancellable when that limit is reached. Fences are not expired automatically.

Rc8 upgrades the coordinator database to schema 2 so older runtimes refuse it
instead of ignoring cancellation fences. Upgrade gateway and runtime together.
Downgrading the desktop to an rc7 bundle stops local operator compatibility;
reinstall rc8 or newer to resume. Never delete the database or restore a snapshot
that predates cancellation acknowledgements to bypass this protection.

The coordinator may use its own keyed RPC endpoint, but participant enrollment
exports only the public Base Sepolia endpoint. Keyed/custom endpoints in incoming
participant enrollment are rejected. Request errors and payment observation
failures must never expose transport URLs, response bodies or private data.
Unknown errors are correlated using a random reference and a fixed category.

## What is not provided

Public marketplace security, confidential computing, malicious-host resistance, Sybil
resistance, Byzantine scheduling, independent GPU usage attestation, OS-enforced memory/
GPU limits, automatic idle detection, malicious model defense, mTLS, credential rotation
without restart, HA coordinator failover, complete rate limiting, durable distributed
storage, secure data erasure, or real payment settlement.

A loaded Ollama model may consume resources outside the worker's declared allowance.
Use dedicated/trusted hardware for this adapter until proper runtime isolation exists.
The worker requires full model GPU residency before and after GPU execution, with a pinned
manifest digest. These are trusted-runtime observations, not independent hardware attestation.
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
