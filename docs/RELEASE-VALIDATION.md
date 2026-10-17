# Soapbox 0.3.0-rc.1 release validation

Validated on 2026-10-17 with Bitterbot Desktop 1.8.0, Ubuntu 24.04 under WSL2,
Node 22.22.1 and an NVIDIA RTX 4080. Buyer and operator used separate gateway profiles
and scoped coordinator identities on one physical host. This is not a two-machine WAN test.

## Paid GPU acceptance

Job `92ff3619-dbed-461d-9f5c-6432a9a8df3e` succeeded on the enrolled `release-operator`.
The isolated container produced two 384-dimensional vectors with the pinned all-minilm model,
reported full GPU residency and completed its first attempt in 78 seconds. Its memory limit
was 1024 MiB with zero swap, no network and a read-only filesystem/model cache.

Base Sepolia (84532) operator USDC increased from 0.037 to 0.039. Repeated payment refreshes
did not pay twice. A separate RPC receipt check confirmed the successful settlement and
exact 2000 micro-USDC transfer to the enrolled operator address.

- [Escrow funding](https://sepolia.basescan.org/tx/0x1d534905f58e196faed27e24e56f9dce866bb2901f8e1e598f900e140649bd25)
- [Operator settlement](https://sepolia.basescan.org/tx/0xeeba1c982d881185cbb8fe6a17b3a41ed90f6eff0b3783aa4db988fd30bf724c)
- [Machine-readable evidence](release-validation.json)

The operator gateway rejected buyer wallet access, job submission, funding, unassigned-job
reads and pausing another worker. Assigned job receipts matched on both desktops. The main
wallet's configuration and identity hash were unchanged; only the separate test account funded work.

## Checks passed

- Rust: 29 unit/lifecycle/network tests, formatting and strict Clippy.
- Docker: both real isolation tests, including resource enforcement and filesystem/network boundaries.
- Gateway/wallet: 14 focused tests; participant enrollment/environment isolation: 3 tests.
- Desktop: 7 Compute component tests and a production overlay build against the exact 1.8.0 release.
- Backend TypeScript check and targeted pre-commit lint.
- Distribution: 3 tests covering scoped enrollment, both installer roles, integrity checks,
  main-wallet preservation and guarded rollback.
- Packaged operator setup with the approved model cache and real isolated NVIDIA GPU probe.

Only Ubuntu/WSL x64 has an installable package. Native Windows/macOS GPU operators, fresh-machine
driver provisioning, a fresh model download and two physical machines remain outside this local
validation. Cross-platform CI results should be checked on the published commit. Workload results
and GPU residency retain the trusted-pilot assurances documented in SECURITY.md.
