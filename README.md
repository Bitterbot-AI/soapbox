# Soapbox

Opt-in compute pools for **paid, private, and collective workloads**, implemented in Rust.
Bitterbot is the first client; the protocol and worker can run independently.

**Release 0.3.0-rc.4 is a trusted pilot, not a public cloud marketplace.** Paid jobs support
Base Sepolia USDC escrow with fixed operator payouts and confirmed settlement. Mainnet is disabled.
The original non-redeemable TEST_CREDIT mode remains available for development.
Storage is an account-scoped temporary artifact service on one coordinator.

The [private desktop wizard release](docs/TEST-RELEASE.md) includes click-to-open setup,
prerequisite checks and guarded rollback for Bitterbot 1.8.0 on Ubuntu/WSL2. The approved
installer and desktop integration are rc4; the approved GPU runtime remains rc3. Compute is visible and enabled
by default; enrollment, worker activation and each payment require user action.
See the [review disposition](docs/RC3-REVIEW-DISPOSITION.md) for the included fixes.

## What runs today

- Authenticated coordinator with persistent SQLite jobs, accounts, reservations, receipts, and artifacts.
- Independent workers connecting over HTTP on loopback or HTTPS remotely.
- CPU SHA-256 batches with independently recomputed results.
- GPU embedding batches through a pinned local Ollama model, automatic warmup, NVIDIA admission checks and runtime evidence.
- Private pool ACLs, collective project ACLs and daily quotas, paid budget reservation and settlement.
- Heartbeats, one active lease per worker, bounded renewal, cancellation, retry and refund.
- Scoped API credentials, operator pause, local pause file, admission limits, and idempotency.
- Bitterbot gateway adapter and desktop Compute view (in the sibling bitterbot-desktop repository).
- End-to-end tests with actual worker processes.

GPU residency is observed from Ollama, not attested. Embeddings are checked for shape,
not semantic correctness. Workers are explicitly admitted and trusted in this release.
The isolated Linux/WSL worker enforces CPU, RAM, zero-swap and process limits. Legacy host
execution is an explicit development option and cannot accept USDC jobs.

Start with [Phase 2 isolation and USDC setup](docs/PHASE-2-ISOLATION-PAYMENTS.md).

The earlier host-runtime increment is documented in [Phase 2 GPU setup and architecture](docs/PHASE-2-GPU.md) and
[physical GPU validation](docs/PHASE-2-VALIDATION.md).
The sibling bitterbot-desktop repository includes `scripts/soapbox-gpu-pilot.ps1`
for development. Testers should use the packaged desktop wizard.

## Quick start

Prerequisites: stable Rust and a C toolchain. Node 22+ is needed only for the demo.

```sh
cargo build --locked
node scripts/demo.mjs
```

The demo starts a coordinator and two workers, submits all three job modes, checks results,
verifies accounting and artifact retrieval, prints a report, and stops its processes.
Every run generates fresh local credentials. It does not contact a payment provider.

To leave the demo available for the Bitterbot UI:

```sh
node scripts/demo.mjs --keep
```

This writes a private, ignored `.local/demo.env` with the three Bitterbot connection settings.
Load those settings into the gateway environment and restart the gateway.
See [the runbook](docs/RUNBOOK.md). Do not commit this file.

If you built into another target directory, set `SOAPBOX_BINARY` to the executable.
For example:

```sh
SOAPBOX_BINARY=/tmp/soapbox-target/debug/soapbox node scripts/demo.mjs
```

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs these checks on Linux, macOS, and Windows. Local verification is recorded in
[VALIDATION.md](docs/VALIDATION.md); CI configuration is not evidence that those hosts passed.

## Documents

- [Architecture and trust boundaries](docs/ARCHITECTURE.md)
- [Phased MVP plan and acceptance gates](docs/MVP-PLAN.md)
- [API and wire examples](docs/API.md)
- [Operations and Bitterbot integration](docs/RUNBOOK.md)
- [Legacy Java mapping](docs/LEGACY-MAPPING.md)
- [Threat model and current limitations](docs/SECURITY.md)
- [Partner pilot briefs](docs/PARTNERS.md)

The repository does not depend on the old Java blockchain or transcoding stack.
The original Java repository remains separate.

## Windows development demo

Build the native executable with `cargo build --locked`, then run the demo from the
repository root in PowerShell:

```powershell
.\scripts\demo.ps1
```

Add -Keep to leave the demo running for the Bitterbot UI. The script restores your prior
SOAPBOX_BINARY setting on exit. This executable is an unsigned debug build; the source
and build instructions are the portable deliverable.

## Installable private test release

See [release validation](docs/RC4-RELEASE-VALIDATION.md) and [buyer and operator setup](docs/TEST-RELEASE.md) for the Ubuntu 24.04 / WSL2 x64 package compatible with Bitterbot 1.8.0.
