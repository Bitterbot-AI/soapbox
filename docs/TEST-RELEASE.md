# Soapbox 0.3.0-rc.2 — desktop wizard preview

This private release installs **Compute → Set up buyer or operator** into an existing
Bitterbot Desktop **1.8.0** installation. Payments are restricted to **Base Sepolia
(84532), test USDC**. Mainnet wallet settings and wallet files are preserved.

## Install without terminal commands

1. Sign in to GitHub with an account granted access to `Bitterbot-AI/soapbox` and open
   the `v0.3.0-rc.2` release. Download the Windows/WSL ZIP or Linux tar.gz asset.
2. Extract the **entire** archive. On Windows, open **Soapbox-Setup.exe**. On Ubuntu,
   open the executable **Soapbox-Setup**. A local setup page opens in your default browser.
3. Choose **Buyer** or **GPU operator**, then **Check prerequisites**. Correct any failed
   prerequisites before continuing. The installer does not change drivers, Docker permissions
   or Windows security settings. This unsigned private preview may be blocked by organizational
   security policy; obtain an approved build rather than disabling those protections.
4. Check the installation acknowledgment and click **Install Compute wizard**. The installer
   verifies all packaged files and backs up the affected configuration before replacing it.
   Existing Compute participants keep their enrollment and worker configuration.
5. Open Bitterbot using the link. Restart Bitterbot normally if it does not reconnect itself.
   Open **Compute → Set up buyer or operator** and complete the role-specific steps below.

Only Ubuntu 24.04 x64 / WSL2 Ubuntu 24.04 is supported. On Windows, Bitterbot must be
installed in your **default WSL distribution**. This is an add-on installer, not a fresh
Bitterbot or WSL installer. The installed Bitterbot supplies Node.js 22+. Buyers need
256 MiB of free disk; operators need at least 4 GiB and accessible Docker with NVIDIA
Container Toolkit and cgroup v2 memory/swap enforcement. A working NVIDIA driver is required.
The setup page binds only to loopback and uses a random, temporary session credential.
Click **Close installer** when finished; unused sessions expire after 30 minutes.

## Buyer

Choose **Buyer** in Compute. Create your separate Base Sepolia wallet using your own CDP
credentials (or the gateway's existing credentials). Give only its public address to the
coordinator owner and import the buyer enrollment JSON they provide privately. Review the
coordinator, identity and wallet address, acknowledge, and click **Activate buyer**.

Obtain Base Sepolia test USDC and test ETH for gas. Select **Paid · test USDC**, an available
operator and **GPU · batch embeddings**. Submit your inputs, review the exact quote and fund
that job separately. The test wallet retains 0.10 USDC/job and 1 USDC/day spending caps.
The installer never funds a job or switches the main wallet to testnet.

## GPU operator

Choose **GPU operator** in Compute and import the coordinator-issued operator enrollment.
Its payout address is a Base Sepolia address you control; no signing key is required.
Click **Prepare GPU runtime**. The approved rc1 runtime is included and its exact SHA-256
is checked again before execution. Setup builds the fixed container image, downloads the
pinned model and verifies isolated GPU execution. No second GitHub download is necessary.

Review the payout address, acknowledge the 1 GiB allowance, and click **Activate operator**.
Compute shows readiness and earnings. **Pause new work** stops new allocations. This GUI
setup enables approved paid test jobs; private/collective memberships require coordinator
authorization and worker configuration. Unsupported hardware fails closed.

## Rollback

Reopen **Soapbox-Setup**, check **Restore the previous Compute setup and keep my wallet data**,
and click **Roll back this installation**. Restart Bitterbot normally if necessary.

Rollback preserves mainnet and test-wallet files, transaction history, models and enrollment
backups. It restores the integration settings owned by this installation and preserves unrelated
settings edited later. If the wizard activated a participant, the coordinator must be reachable
and visible pending jobs must finish/cancel and settle before rollback. Conflicting changes to
Compute block automatic rollback; the private `before-config.json` remains in the deployment
directory for recovery. No user data is deleted. The most recent installer deployment is selected.

## Coordinator and network preparation

The coordinator owner must enroll the test user's public address and provide a scoped JSON file.
Never share an admin token, CDP secrets or private wallet key. Enrollment management remains an
administrator task; this release does not expose a public enrollment service. Configure a valid
HTTPS coordinator endpoint or an agreed private tunnel before testing from another machine.
No firewalls, DNS, VPNs or public ports are changed by this installer.

## Release boundaries

The new installer/UI/plugin version is rc2; the bundled fixed-workload Rust runtime remains the
previously verified rc1. SHA256SUMS and the per-file release manifest detect corruption; these
are not a publisher signing certificate. No credentials, enrolled identities, wallet files,
databases, logs or cached models are distributed. Only approved CPU hashing and GPU embedding
workloads run. GPU evidence remains worker-reported and embedding verification checks shape.
Physical two-machine acceptance and independent security review remain separate gates.
