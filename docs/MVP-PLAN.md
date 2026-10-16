# Phased MVP plan

Planning baseline: 2026-10-16. Stages are acceptance gates, not promised delivery dates.
The implementation delivered with this plan completes the v0.1 trusted pilot slice.
Later stages need hardware, buyer specifications and operational decisions.

## Phase 0 — Contracts and legacy extraction (implemented)

Deliverables:
- Standalone Rust repository and typed API v1 contracts.
- Ownership, identity, contribution modes and resource semantics.
- Legacy mapping; no Java blockchain, emission schedule or video dependencies.

Exit gate: protocol and state transitions documented; unauthorized private/collective
access is rejected; temporary storage and test economics are clearly labelled.

## Phase 1 — Trusted execution pilot (implemented)

Deliverables:
- Persistent coordinator, provisioned identities and explicit worker admission.
- CPU workload; optional configured-model embeddings adapter.
- Paid TEST_CREDIT flow, private pools, collective project quotas.
- Atomic reservations, cancellation/refund, lease recovery and idempotent settlement.
- Temporary artifacts with owner isolation, byte quotas and TTL.
- CLI, multi-process demo, gateway adapter and desktop Compute view.
- Automated failure-path tests and CI configuration.

Exit gate:
1. Two independent worker identities complete paid/private/collective jobs.
2. Invalid/late results never settle; duplicate requests never duplicate charges.
3. Worker loss retries within limits; final failure returns reserved credits.
4. Restart preserves jobs and accounting; cross-pool access and artifact access fail.
5. The desktop can submit, inspect, cancel, pause new allocations and retrieve artifacts.

Limitations: trusted workers, central authority, one slot per worker, fixed input-item
test pricing, no live payments, no general code execution, no automatic mesh/circle federation.
No claim of native Windows/GPU validation unless actually tested on those environments.

## Phase 2 — Qualified CPU/GPU service and a paid customer pilot

Dependencies: one buyer workload contract; designated supported GPUs/OSes; partner demand.
Suggested scope: a small opt-in fleet with bounded batch jobs, not all existing mesh nodes.

Build:
- Hardware inventory and reproducible CPU/GPU benchmarks, compatibility and expiry.
- Explicit availability windows, power/thermal limits, host activity policy and manual overrides.
- Trusted runtime images with model digests and runner/backend version binding.
- Job partitioning and aggregation; bounded concurrency, per-tenant fairness and backpressure.
- Isolation appropriate to each workload; OS-enforced CPU/RAM/GPU budgets.
- Result acceptance appropriate to task: recomputation, redundancy/challenges, trusted hardware,
  or contractually explicit trusted-provider assurance.
- Accurate resource/time meters, pricing quotes with expiry, refunds and failure attribution.
- Signed receipts and replay protection; real payment reservation/capture/release/reconciliation.
- Buyer spend limits and provider payout controls; no fabricated live-escrow implementation.
- Operator installer/updater, signed binaries, revocation, crash recovery and telemetry.

Exit gate:
- Agreed workload meets buyer's throughput, success, latency and data-placement requirements.
- Measured gross revenue covers operator payouts, transfer, retries, verification and operations.
- Chaos tests cover worker/coordinator/network loss and settlement-provider timeouts.
- Payment reconciliation balances actual funds; no double capture or payout under response loss.
- Independent security review for exposing the service beyond a trusted pilot.

Pilot decision: if buyers need reserved datacenter GPUs, qualify that subset separately.
A large consumer-node count is not a substitute for compatible available capacity.

## Phase 3 — Storage and data delivery

Dependencies: measured storage demand, durability objective, retention obligations and recovery cost.

Build:
- Encrypted content-addressed object API, multipart/resumable transfers and access capabilities.
- Explicit distinction between evictable cache and durable objects.
- Placement across independent failure domains, replicas/erasure coding, integrity audits and repair.
- Storage reservations, graceful exit, repair budgets and network egress accounting.
- Data residency policy, key ownership, deletion and recovery procedures.
- Cached model/artifact distribution for compute jobs.

Exit gate: controlled node-loss experiments meet the declared durability/recovery objective;
accounted usable capacity includes redundancy, repair space and bandwidth costs.
Separate benchmarks for tiny-object metadata load and large-object throughput.

## Phase 4 — Persistent agents and private enterprise fleets

Dependencies: reliable execution and storage from earlier stages.

Build:
- Persistent workspaces, checkpoint/restart, image lifecycle and migration.
- Private fleet administration, tenant quotas, delegated roles and audit exports.
- Cognitum/ruOS adapter if jointly validated; allow other runtimes.
- Bitterbot agent placement policy for local/private/public execution.
- Circle membership adapter with signed grants, expiry, revocation and bounded disclosure.
- Subscription and usage billing for actual availability commitments.

Exit gate: an agent survives host loss and resumes without leaking credentials or crossing
tenant boundaries; an enterprise operator can revoke access and audit every placement.

## Phase 5 — Federated marketplace and collective scale

Build:
- Signed, expiring resource discovery on the existing mesh.
- Cross-coordinator identity, reservations and nonduplicated capacity commitments.
- Reputation based on measured service delivery and resistance to fabricated identities.
- Marketplace offers, SLA classes, disputes and auditable settlements.
- Collective scheduling governance, contribution credits and fair-use budgets.
- Partner adapters validated against actual buyer and hardware qualification contracts.

Exit gate: scale/abuse/load tests on measured fleet inventory; market revenue from real demand;
documented operator support and recovery procedures. No automatic enrollment of existing nodes.

## Measures from day one

Track opted-in/eligible/online workers separately; compatible GPU VRAM and CPU/RAM;
queue delay; accepted jobs; retries and wasted work; success rate; actual bytes transferred;
operator interruption; contribution consumption; reservation/refund age; accepted cost per job.
Avoid using registered mesh peers as a saleable-capacity metric.
