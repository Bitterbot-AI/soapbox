# Soapbox v0.3.0-rc.5 validation

Validated on 2026-10-17 on Ubuntu 24.04 / WSL2 with an NVIDIA RTX 4080. This is a public
prerelease for Bitterbot Desktop 1.8.0. Compute remains on by default. Paid jobs remain
on Base Sepolia (84532), with the Compute wallet separate from the mainnet wallet.

## What changed

- Signed Compute discovery and private/collective batches use Bitterbot's existing P2P
  identity, encrypted transport and shared request dispatcher.
- One-click operator setup prepares a checksum-pinned runtime/model, checks isolation and
  GPU execution, then activates a supervised loopback coordinator and worker. No wallet or
  enrollment file is needed for local private/collective hosting.
- Operators grant and revoke peer access to private pools and collective projects.
- The installer packages matching gateway, UI, plugin, worker and P2P sidecar builds.
  Prerequisites include Docker/NVIDIA, cgroup v2, enforced swap limits and disk space.
  Rollback restores owned gateway files and P2P configuration while preserving wallet data.
- Existing paid buyer/operator enrollment and per-quote approval remain available.

## Executed checks

| Check | Result |
| --- | --- |
| TypeScript type checking | Passed |
| Staged source lint with type awareness | Passed, no warnings/errors |
| Setup, protocol, batching and peer transport | 27 tests passed |
| Gateway authorization | 1 test passed |
| Compute setup, dashboard and mesh UI | 15 tests passed |
| Installer integrity, conflict handling and rollback | 11 tests passed |
| Real operator supervisor recovery after coordinator termination | Passed |
| Gateway, plugin, UI and optimized Rust builds | Passed |
| Fresh profile install and one-click activation through the packaged UI | Passed |
| Separate P2P buyer: private CPU, collective CPU and GPU embeddings | Passed |
| Denial before admission and after revocation | Passed |
| Activated operator rollback: gateway and P2P restored, dummy wallet hash unchanged | Passed |

The packaged peer test completed three batches: two CPU inputs in private mode, two in
collective mode and two GPU embedding inputs. The RTX 4080 worker ran the pinned adapter
inside containers with networking disabled. The UI displayed completed jobs, GPU readiness
and operator controls. The final UI copy was rebuilt and inspected after the acceptance run;
gateway/plugin/Rust executable assets retain the tested hashes.

The preceding mesh increment also passed 34 Soapbox Rust tests and 13 Rust orchestrator
tests, plus a three-node local fan-out/restart test (40 CPU inputs per batch and two GPU
embedding rows). Two Docker-boundary tests remain ignored by the default Rust suite; the
real container acceptance flow does not replace all of their boundary assertions.

No payment was made by the rc5 acceptance test. Both P2P identities used the same physical
computer. This is not a two-machine, NAT/relay, fleet-scale or paid-mesh acceptance result.

## Provenance and public-source review

Each package contains `release.json` with exact Soapbox/desktop commits and per-file hashes;
the release provides `SHA256SUMS` for the Linux, Windows/WSL and standalone runtime archives.
The supported desktop base is commit `619ca967fe4d8a75142ec064ba00bb917efd3012` (v1.8.0).
The runtime archive includes source-file hashes in `provenance.json` and is pinned by
`distribution/runtime.json` and the matching desktop setup code.

Gitleaks 8.30.1 scanned both available Git histories and the unpacked candidate (128 MB).
The 116 package matches were reviewed as public addresses, dependency examples/constants,
variable names and PEM formatting code; no local wallet, enrollment or gateway credentials
were identified. Credentials, logs, profiles, databases and models are not package inputs.
Environment-file variants, wallets and private node keys are ignored by Git.

An unverified token-shaped value in imported historical desktop benchmark data remains a
separate review item. It is absent from the current checkout and these artifacts. A secret
scan is not a proof of absence, and removing a historical value does not revoke it.

## Remaining limits

Trusted operators can see job inputs. GPU result verification checks shape and pinned
runtime/model evidence; it is not independent hardware attestation. Paid P2P fan-out,
storage durability, verified replicas, physical-host/NAT acceptance and production publisher
signing remain future work. The unsigned launchers are unchanged from rc4.

During validation, WSL was restarted with user approval. The main Bitterbot gateway and
Docker were restored; Docker resource controllers and non-root cgroup placement were
restored before tests. Prerequisite checks correctly refused installation while swap-limit
support was unavailable. Isolation checks were not bypassed.
