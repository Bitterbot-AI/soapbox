# API v1

> Current isolation and USDC extension: [Phase 2 isolation and payments](PHASE-2-ISOLATION-PAYMENTS.md). Earlier TEST_CREDIT/shared-host descriptions below refer to the original pilot.

All routes require Authorization: Bearer <credential>. The JSON API has no CORS browser
interface. Bind to loopback or put the service behind a TLS reverse proxy on a trusted network.
Errors are JSON { "error": "..." } except malformed extractor/body errors from the HTTP framework.

## Routes

| Method | Route | Required authority |
|---|---|---|
| GET | /v1/status | Any authenticated principal; private offers filtered, jobs owner-scoped unless admin |
| GET | /v1/jobs | Own jobs, or all for admin; latest 100, result bodies omitted |
| GET | /v1/jobs/{id} | Job owner or admin |
| POST | /v1/jobs | submitter plus relevant pool/project membership |
| POST | /v1/jobs/{id}/cancel | submitter and owner/admin |
| POST | /v1/workers/register | worker; replaces offer and refreshes heartbeat |
| POST | /v1/workers/claim | worker; one active lease per identity |
| POST | /v1/jobs/{id}/renew | worker holding current, unexpired lease |
| POST | /v1/jobs/{id}/complete | worker holding current lease; identical replay accepted |
| POST | /v1/jobs/{id}/fail | worker holding current lease |
| POST | /v1/workers/{id}/pause | Same worker or admin |
| POST | /v1/credits | admin; TEST_CREDIT only, no cash/redemption |
| POST | /v1/artifacts | submitter; UTF-8 content, quota and TTL |
| GET | /v1/artifacts/{id} | Artifact owner, before expiry |

## Job request

```json
{
  "idempotency_key": "buyer-defined-unique-intent",
  "mode": { "kind": "private", "pool_id": "circle-demo" },
  "workload": { "kind": "sha256", "texts": ["one", "two"] },
  "resources": { "cpu_threads": 1, "memory_mib": 256, "require_gpu": false }
}
```

Other modes: { "kind": "paid", "max_cost_credits": 2 } and
{ "kind": "collective", "project_id": "research-demo" }.
Current fixed quote is one test credit per input string. Reserve the quote, not the full cap.

Embedding workload:
{ "kind": "embeddings", "model": "all-minilm:latest", "model_digest":
"1b226e2802dbb772b5fc32a58f103ca1804ef7501331012de126ab22f67475ef", "texts": ["one"] }.
The exact model must be installed and digest-pinned on the worker; v0.2 warms it automatically.
GPU requests must include workload.model_digest (64 lowercase SHA-256 hex characters).
GPU jobs require full model residency before and after execution plus worker-reported evidence.
See PHASE-2-GPU.md for the execution object included in results and receipts.

The same idempotency key with different content is a conflict. A new intentional job
requires a new key. Retry an uncertain submission with the same key.

## Completion and renewal

Claim returns null or a Job with id, spec, lease_id, lease_until and attempts.
Renew/fail: { "lease_id": "..." }.
Complete: { "lease_id": "...", "result": { "hashes": ["..."] } }.
An unpinned legacy embedding result may contain only embeddings. A pinned result must also
include the typed execution evidence described in PHASE-2-GPU.md.

Results are capped at 2 MiB. Accepted completion returns a receipt indicating the actual
assurance: recomputed or shape_only_trusted_worker. Paid settlement is a fixed accepted-job
charge; elapsed time is informational.

## Administrative and artifact requests

Credit:
{ "account": "buyer", "amount": 100, "idempotency_key": "unique-funding-intent" }.
The same funding intent cannot be reused with another account or amount.

Pause:
{ "paused": true }. Only new allocations are paused; existing work can finish.

Artifact:
{ "content": "UTF-8 text", "ttl_seconds": 3600 }.
Returns id, sha256, bytes, expires_at and durability=temporary_single_coordinator.
Artifacts are owner-only and are not automatically inputs for jobs in v0.1.

## Compatibility

The status response advertises api_version=1, settlement=test_credits and capabilities.
Incompatible payload semantics require a new API version. Unknown submitted fields are
rejected. Clients must preserve idempotency keys on retry and inspect capabilities rather
than infer GPU, cash settlement or durable storage from the service name.
