# Soapbox 0.3.0-rc.1 — private test release

This release connects a buyer's Bitterbot Compute view to an explicitly enrolled operator.
Payments use **Base Sepolia (84532), Circle test USDC only**. The installed main wallet,
its network, funds, spending limits and identity files are preserved. Test tokens have no monetary value.

Supported package: **Ubuntu 24.04 x64 / WSL2 Ubuntu 24.04, Bitterbot Desktop 1.8.0**.
Node.js 22+ is required. An existing Bitterbot installation includes Node at
`~/.bitterbot/app/current/runtime/node/bin/node`; use that full path if `node` is unavailable.
The installer deliberately refuses other desktop versions. Native Windows/macOS GPU workers
and automatic P2P discovery are not included. Two physical machines still need a joint acceptance test.

## Download and verify (both roles)

Your GitHub account must have access to the private `Bitterbot-AI/soapbox` repository.
Authenticate `gh` using your own account; never share a personal access token.

```sh
gh release download v0.3.0-rc.1 --repo Bitterbot-AI/soapbox --dir soapbox-download
cd soapbox-download
sha256sum -c SHA256SUMS
tar -xzf soapbox-0.3.0-rc.1-linux-x64.tar.gz
cd soapbox-0.3.0-rc.1-linux-x64
```

The archive contains the Rust binary, compatible desktop plugin/UI, installer and GPU setup tool.
No wallet credentials, enrollment tokens, participant identities, databases, models or local logs
are included. Checksums detect corruption; this is an unsigned prerelease.

## Buyer setup

The buyer needs their **own** CDP wallet configuration in Bitterbot. The enrollment file never
provides CDP credentials. If you do not already have a test account, initialize it locally:

```sh
node --env-file="$HOME/.bitterbot/.env" plugin/buyer-wallet.cjs
```

This prints a separate Base Sepolia account address and stores its public identity in
`~/.bitterbot-soapbox/wallet/base-sepolia`. If credentials are configured directly in
Bitterbot instead of `.env`, omit `--env-file` (the wallet secret must still be in your environment).
Request faucet test USDC and test ETH for this address. Funding escrow needs gas.
Send **only this public address** to the coordinator operator to receive a private `buyer.json` enrollment.

```sh
node setup/install.mjs --role buyer --connection /absolute/path/buyer.json
node setup/install.mjs --role buyer --connection /absolute/path/buyer.json --apply
```

Restart Bitterbot normally. Open **Compute**, confirm the Base Sepolia address, select
**Paid · test USDC**, choose the enrolled GPU operator, then **GPU · batch embeddings**.
Submit two lines. Review the exact 0.002 test-USDC quote and payout address, then fund it.
The dedicated wallet is limited to 0.10 test USDC per job and 1 test USDC per day.

## Operator setup

The operator needs NVIDIA GPU support, Docker with the NVIDIA Container Toolkit and enforced
memory/swap limits. Docker must work as the current user. Under Windows, run these commands
inside the WSL Ubuntu distribution where Bitterbot runs. Hardware and container setup must
be completed before enrollment. No CDP credentials or buyer wallet are needed on this node.

```sh
node setup/setup-operator.mjs
```

Setup builds the pinned workload image, downloads the approved `all-minilm:latest` model,
checks its expected digest and runs an isolated GPU probe. A changed upstream model digest
fails closed; never change the pin just to bypass the check. The download has network access;
actual workload containers have no network access. Allow at least 4 GiB free disk and 1 GiB
available RAM, plus the GPU/model allowance. Stop competing GPU workloads for the probe.
For an existing approved cache, pass `--model-cache /absolute/path/models`.

Give the coordinator a public EVM address where you want to receive **Base Sepolia test USDC**.
Verify you control that address; its private key stays in your wallet. Obtain `operator.json`, then:

```sh
node setup/install.mjs --role operator --connection /absolute/path/operator.json \
  --worker-config "$HOME/.bitterbot-soapbox/operator/worker.json"
node setup/install.mjs --role operator --connection /absolute/path/operator.json \
  --worker-config "$HOME/.bitterbot-soapbox/operator/worker.json" --apply
```

Restart Bitterbot. Compute shows your operator ID, GPU readiness, payout address, assigned
jobs and confirmed earnings in the most recent 100 jobs. **Pause new work** drains existing work.
Private and collective participation additionally require explicit pool/project lists in the
worker configuration and matching coordinator ACLs; paid participation is enabled by default.

## Coordinator enrollment (Victor / pilot administrator)

The existing trusted pilot owns escrow settlement. Start its updated desktop integration
and Soapbox 0.3.0-rc.1 binary before enrolling participants. Enrollment writes persistent
`admissions.json`; the coordinator loads it at startup. The relayer retains its own signer.

```sh
node setup/enroll.mjs --state "$HOME/.bitterbot-soapbox-pilot" \
  --role operator --id tester-gpu --address 0xPUBLIC_PAYOUT_ADDRESS \
  --url https://YOUR_COMPUTE_HOST --output /private/operator.json
```

For a buyer, use `--role buyer`, a different ID, and their separately initialized test-wallet address.
Optional `--pool circle --project science` grants those memberships. Enrollment only exports
that participant's token and public payment configuration, never admin or wallet credentials.
Restart the pilot after changes. To revoke a participant, remove its row from `admissions.json`
and restart the coordinator; let already funded work settle/refund before revocation.

**Transport:** use a valid HTTPS reverse proxy to the coordinator's loopback port 8787,
or a prearranged SSH tunnel. Do not expose plaintext port 8787 or the Bitterbot gateway.
For SSH: on the participant, run `ssh -N -L 127.0.0.1:8787:127.0.0.1:8787 USER@COORDINATOR`;
enroll with `--url http://127.0.0.1:8787`. Verify the SSH host key out of band. Keep the tunnel
running. If the coordinator is behind NAT, arrange the VPN/tunnel before the test.
This release does not open firewalls, configure DNS or discover peers automatically.

## Acceptance checklist

1. Buyer and operator show different principal IDs; verify the operator's public payout address.
2. The chosen operator is online, isolated, and has passed its GPU probe.
3. Submit/fund a two-line GPU job. The quoted operator must execute it; an unfunded job must not run.
4. Both desktops show the same job ID and accepted receipt; the result has two 384-dimensional vectors.
5. The operator's Base Sepolia USDC balance increases by exactly 0.002. Check the escrow settlement
   and token transfer on Base Sepolia's explorer. A local credit count alone is not payment evidence.
6. Refresh/restart the worker and confirm there is no second payout. Check both main wallets remain unchanged.

## Rollback and diagnostics

Installer preflight does not change configuration. Apply creates a private versioned directory
under `~/.bitterbot/compute/` and prints its path. It retains `before-config.json` there.

```sh
node setup/install.mjs --rollback /absolute/path/printed/deployment
```

Restart Bitterbot after rollback. Rollback refuses to overwrite configuration changed since install;
merge the retained backup in that case. It does not erase test wallets, jobs or receipts.
Operator logs are `worker.log` in the printed deployment directory. Enrollment files contain
credentials: transfer them privately, keep mode 0600, and never attach them to issues.

This is a trusted pilot with coordinator-authorized settlement. GPU residency is worker-reported,
embedding outputs are shape-checked, and the escrow has not undergone an independent security audit.
Only the fixed CPU hashing and GPU embedding workloads are enabled; arbitrary customer containers
and commercially durable storage are outside this release.
