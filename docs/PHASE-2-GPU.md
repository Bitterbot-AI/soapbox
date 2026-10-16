# Phase 2 GPU execution

Implemented and physically tested on 2026-10-16. This is the GPU service increment of
Phase 2; the paid customer/production exit gate in MVP-PLAN.md remains open.

## Supported pilot

- Windows x64 + NVIDIA, tested on RTX 4080 16 GiB, driver 610.88.
- Rust coordinator and worker; local Ollama 0.40.5 CUDA inference.
- all-minilm:latest, 384 dimensions, model manifest SHA-256:
  `1b226e2802dbb772b5fc32a58f103ca1804ef7501331012de126ab22f67475ef`.
- Approved embedding batches in private pools, collective projects and paid TEST_CREDIT mode.
- CPU SHA-256 and artifact APIs remain available. No arbitrary binaries, containers or buyer URLs.

Linux NVIDIA should use the same protocol and local runtime, but physical GPU validation
here covers Windows only. AMD, Apple Metal, distributed training and tensor parallelism
are not qualified by this implementation.

## Architecture

```mermaid
sequenceDiagram
  participant UI as Bitterbot Compute
  participant GW as Bitterbot gateway
  participant C as Soapbox coordinator
  participant W as Opted-in Rust worker
  participant R as Local Ollama / CUDA
  W->>R: Check installed digest, warm model, inspect full residency
  W->>C: Offer model pin, runtime and NVIDIA telemetry
  UI->>GW: Submit mode, membership, pinned model and inputs
  GW->>C: Authenticated submission
  C->>C: Check ACL/quota/budget; reserve test credits
  W->>C: Claim compatible lease
  W->>R: Up to 16 inputs per inference batch
  W->>C: Renew lease while running
  W->>R: Recheck digest, version and GPU residency
  W->>C: Ordered vectors plus execution evidence
  C->>C: Validate binding/shape; settle exactly once
  UI->>GW: Poll selected job
  GW->>C: Read result and receipt
```

The HTTP API stays v1 with additive optional fields. GPU requests now require
`workload.model_digest`; old boolean-only GPU offers cannot satisfy a pinned request.
CPU-only v0.1 jobs remain compatible. Old GPU worker configs must add `ollama_digest`.

The worker checks locally installed model manifests before warmup and before/after execution.
It never invokes a pull endpoint. Only operator setup downloads models.
Both observations must report positive model bytes with all bytes resident on GPU.
NVIDIA telemetry must be available and satisfy admission policy. Each worker runs one lease.
The desktop chooses from online, unpaused models matching the selected mode and membership.

Inputs retain the existing 128-item / 256 KiB bound. Inference chunks contain at most
16 items; aggregation preserves order. Truncation is disabled. Response bytes are capped
at 2 MiB. Entire execution has a 100-second worker timeout inside the coordinator's
120-second attempt limit. This is local batching within one worker, not multi-node fan-out.

## Admission and cancellation

`examples/worker-gpu.json` sets defaults of temperature below 80 C, utilization at most
40%, and at least 256 MiB free VRAM. CPU RAM admission remains separate. A pause file
prevents new claims. GPU checks occur before admission and execution; these are not hard
power, RAM or thermal controls, keyboard-idleness detection, or continuous safety limits.
Multi-GPU hosts conservatively require all observed GPUs to meet admission policy;
there is no per-device assignment yet.

Missing telemetry/model, changed digest, partial residency, busy GPU or warmup failure
withholds the GPU offer and exposes a diagnostic. CPU offers can remain available.
No silent CPU fallback is accepted for GPU-required jobs.

Cancellation revokes acceptance and refunds the reservation. When renewal is rejected,
the worker drains its current bounded request before reconnecting and cannot settle that
lease. Ollama is a shared external runtime: a disconnected/timed-out HTTP request or a
killed worker does not prove immediate physical cancellation. Use a dedicated runtime
for the trusted pilot. Hard process/GPU isolation and reliable abort remain production work.

## Evidence and accounting

Receipts include model digest, runtime version, GPU identity/driver, model/VRAM bytes,
dimensions, batch count, wall milliseconds, backend durations and prompt tokens.
Measurements are worker-reported; they are not hardware attestation or a signed receipt.
Embedding validation checks finite values, shape, count and pinned execution metadata.
It does not prove semantic correctness against a malicious admitted worker.

TEST_CREDIT pricing remains one credit per input, independent of measured GPU time.
Durations are telemetry, not a claim of billable GPU seconds or energy consumption.
No money changes hands; no USDC, escrow, payout or partner integration is enabled.

## Setup and run on this machine

The full local launcher is `scripts/soapbox-gpu-pilot.ps1` in bitterbot-desktop.
It starts a dedicated runtime if needed, validates all modes and prints the desktop URL.
Add `-TestOnly` for automated verification and cleanup. It restores its environment and
stops only the runtime it started. Existing cached model pins are checked without a registry call.
Detailed evidence: [PHASE-2-VALIDATION.md](PHASE-2-VALIDATION.md).


From PowerShell in D:\Bitterbot\soapbox:

```powershell
.\scripts\setup-gpu.ps1
.\scripts\start-gpu-runtime.ps1
```

Setup uses the official portable Ollama release (about 1.5 GB), checks its archive
SHA-256 on installation, installs the small model, verifies its expected digest, and
creates ignored `.local/worker-gpu.json` without overwriting existing operator settings.
The runtime binds loopback, disables cloud, uses a separate model directory and one
parallel inference slot. Keep that terminal open; Ctrl+C stops the runtime.
Alternatively `setup-gpu.ps1 -KeepRuntime` starts an owned background runtime and prints
its PID. Existing runtimes are not stopped by setup.

In another terminal:

```powershell
.\.local\bin\soapbox.exe inspect --config .\.local\worker-gpu.json
```

Use the existing coordinator provisioning in RUNBOOK.md. Set
`BITTERBOT_SOAPBOX_WORKER_CONFIG` to the absolute GPU config path and
`SOAPBOX_WORKER_TOKEN` to its provisioned worker credential. Pool/project names must
match coordinator memberships. Start/restart the Bitterbot gateway with these settings.
Open Advanced > Compute, choose GPU batch embeddings, a ready model, and a mode.
The selected result refreshes until terminal; the receipt contains execution evidence.
Bitterbot does not enroll other existing mesh nodes automatically.

## Reproduce the real gateway test

With the runtime running, from bitterbot-desktop:

```powershell
$env:SOAPBOX_BINARY = 'D:\Bitterbot\soapbox\.local\bin\soapbox.exe'
$env:SOAPBOX_REPORT = 'D:\Bitterbot\soapbox\.local\gpu-validation.json'
node --import tsx scripts/soapbox-smoke.ts --gpu
```

On this checkout, dependencies were installed under WSL. Native Windows testing also needs:

```powershell
$env:ESBUILD_BINARY_PATH = 'D:\Bitterbot\soapbox\.local\esbuild\package\esbuild.exe'
$env:NODE_PATH = 'D:\Bitterbot\bitterbot-desktop\node_modules\.pnpm\follow-redirects@1.15.11_debug@4.4.3\node_modules'
```

These are local toolchain accommodations, not product configuration. A normal native
dependency installation provides its own platform packages.

The smoke test starts isolated coordinator/worker processes and the real gateway with
unrelated subsystems disabled; uses authenticated WebSocket RPC; runs 33 embeddings
per mode (three inference batches); compares a vector from each job with direct inference;
checks 100 -> 67 credits, cancellation/refund, artifacts and shutdown. No existing user
gateway configuration is changed. `--gpu --keep` additionally serves the built desktop
renderer and prints a local URL for interactive testing, with fresh temporary credentials.
The pilot ends on Ctrl+C or after one hour; it is not persistent operator provisioning.

## Remaining Phase 2 gates

Buyer workload contract and SLA; representative sustained benchmarks and cost model;
multi-worker partition/fairness; model/runtime distribution and revocation; per-device
placement; process isolation and hard budgets; signed receipts; real settlement and
reconciliation; signed installers/updates; independent security review and customer pilot.

Sources: [Ollama embed API](https://docs.ollama.com/api/embed),
[running-model API](https://docs.ollama.com/api/ps),
[Windows portable runtime](https://docs.ollama.com/windows),
[all-minilm model](https://ollama.com/library/all-minilm).
