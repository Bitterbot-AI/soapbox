# Soapbox 0.3.0-rc.6 — Branded Compute preview

This public test release adds the original SoapBox wordmark to the Compute dashboard and
buyer/operator setup wizard, with responsive layout and contrast in both themes. It retains
rc5's one-click GPU operator setup and existing Bitterbot P2P transport integration for
Bitterbot Desktop 1.8.0 on Ubuntu 24.04 x64 / WSL2 Ubuntu 24.04.
Compute remains enabled by default. Paid jobs remain on Base Sepolia (84532), using the
separate Compute wallet. The installer does not fund jobs or change mainnet wallet data.

## Install

1. Download the Windows/WSL ZIP or Linux tar.gz from the rc6 GitHub release and extract it.
2. Open **Soapbox-Setup.exe** on Windows, or **Soapbox-Setup** on Linux.
3. Choose Buyer or GPU operator, run the prerequisite checks, and install.
4. Restart Bitterbot normally so the updated gateway and P2P sidecar load. Open **Compute**.

The unsigned Windows launcher starts the local installer inside your existing WSL Ubuntu
environment. Organizational execution policies may block unsigned applications; the package
does not change those policies. Node.js 22 and Bitterbot 1.8.0 must already be installed.
GPU operators need Docker with NVIDIA support, cgroup v2 and swap limits, plus 4 GiB free
disk. Checksum and per-file manifest verification detect corruption; they are not a publisher
signing certificate.

## Host a P2P GPU operator

In **Compute → Set up buyer or operator**, choose **GPU operator**, then click
**Enable this GPU operator**. No enrollment JSON, wallet or terminal commands are required.
Setup verifies the bundled runtime, checks prerequisites, prepares the pinned model, tests
isolated GPU execution and starts a local coordinator. The first setup may take several minutes.
Settings are activated only after readiness succeeds; a failed setup leaves the previous app
configuration intact and displays a retryable error.

The operator permits one CPU thread and 2 GiB of host memory per job. It serves the private
pool **private** and collective project **community**, with a daily collective allowance of
1,000 items. No remote peer can submit until you grant its full Bitterbot peer ID access in
**P2P Compute**. Discovery is public metadata; job inputs are sent only to selected operators.
Those operators can see the inputs they execute. Use **Pause new work** to stop allocations.

In another updated Bitterbot app, select the discovered operator, matching pool/project and
CPU hashing or GPU embeddings in **P2P Compute**. Submit the batch and inspect progress and
results. The Compute screen and P2P dashboard share the same activity view. Offline cancellation
stays pending until the peer responds. GPU results currently require a trusted operator.

## Paid Base Sepolia pilot

The existing paid flow remains available. Buyers create a separate Compute test wallet using
their own CDP credentials and import a scoped coordinator enrollment for that wallet address.
Review and fund each job explicitly. Existing caps remain 0.10 test USDC per job and 1 test USDC
per day, with test ETH for gas. Never share private keys or CDP secrets with a coordinator.

Operators joining that pilot select **Connect to an existing coordinator for paid Base Sepolia
jobs instead**, import their operator enrollment and prepare/activate the runtime. The enrollment
contains their payout address; no wallet signing key is needed. This is distinct from the local
private/collective pool. Paid multi-operator execution over P2P is not included in this release.

## Rollback

Reopen **Soapbox-Setup**, select rollback and restore the previous installation. Restart
Bitterbot normally. The installer restores its gateway files, UI/plugin paths and P2P sidecar
settings, while retaining wallet files, databases, model caches and enrollment backups.
It preserves unrelated settings and refuses to overwrite gateway/P2P changes made elsewhere.
An activated operator's coordinator must be reachable and unfinished jobs must complete or be
cancelled before rollback. Backups stay under the local `compute` installation directory.

## Test scope and limitations

The release includes the branded rc6 UI/plugin metadata and the unchanged rc5 gateway,
plugin executable code, Rust P2P sidecar and Rust worker. The standalone runtime archive
keeps its rc5 filename and checksum; it is included in both installers and this release.
Only approved CPU hashing and GPU embedding adapters execute. Discovery, admission, fan-out,
restart recovery and isolated GPU execution are tested locally. Physical two-machine, NAT/relay
and fleet-scale acceptance remain separate gates. Relay nodes carrying Compute gossip also need
the updated topic allowlist. Storage durability, verified GPU replicas and paid mesh settlement
are later work. This is a trusted test network, not an arbitrary-code public cloud service.

No wallet files, credentials, enrollment secrets, local databases, logs or cached models belong
in this release. Public contract addresses, transaction hashes, model hashes and source code are
safe to inspect. See the release notes for validation results and source commit provenance.
