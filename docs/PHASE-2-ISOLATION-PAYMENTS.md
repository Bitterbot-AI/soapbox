# Phase 2: isolated execution and USDC settlement

This increment supplies the engineering path for a trusted paid pilot. Public production/mainnet activation, an independent contract/security audit, and commercial SLA qualification remain separate gates. Base Sepolia uses Circle test USDC, which has no monetary value. Local EVM tests use an explicitly labelled TestUSDC fixture.

## Execution boundary

Each CPU or GPU job gets a fresh, immutable Linux container image containing the Rust runner and a digest-pinned Ollama runtime. Buyers supply bounded SHA-256 inputs or embedding batches; they cannot submit images, commands, environment variables, mounts, URLs or model downloads.

The executor enforces a read-only root, non-root UID 65532, all capabilities dropped, no-new-privileges, default seccomp, no network, a read-only model cache, a 128 MiB noexec/nosuid/nodev temporary filesystem, PID/descriptor/core limits, CPU quota, RAM limit and zero swap. Only the model cache is mounted; no wallet credentials, host home, Docker socket or job directory enters the sandbox. The runner checks actual cgroup values, UID, seccomp, capabilities, mount and network state before accepting work. Unsupported limits fail closed.

An OS file lock serializes access to all GPUs assigned to this worker host. Configure every worker on that host with the same gpu_lock path. Containers are removed on normal completion or cancellation, and the in-container 100-second deadline survives a disconnected worker. Losing a lease cancels the container before another job may claim the GPU. The RTX 4080 does not supply a hardware VRAM partition: exclusive Soapbox admission, model/input allowlists and NVIDIA admission checks bound this pilot. Containers share the Linux kernel and GPU driver; they are not confidential-computing VMs or suitable for arbitrary hostile GPU code.

Private and collective modes use the same sandbox, with existing pool/project ACLs. Legacy shared-host execution requires explicit allow_unsafe_host=true and cannot execute USDC jobs.

## Payment flow

1. Buyer submits paid_usdc with operator ID and max_cost_microusdc. Amounts are integer USDC micro-units (six decimals).
2. Coordinator creates an idempotent awaiting_payment job and immutable quote binding job content, chain, escrow, provisioned buyer/operator wallets, price and expiry. No test-credit reservation is made.
3. Bitterbot displays the exact amount and payee. A separate funding action uses the Base Sepolia wallet, existing spending caps, a persisted spend reservation, an exact-amount token approval and escrow funding. Escrow address and deployed bytecode hash must be pinned by the gateway operator. Mainnet is rejected.
4. Rust independently verifies RPC chain ID, escrow bytecode, token, buyer, operator, amount and expiry at a confirmation-depth block, checking that block hash remains stable. Only confirmed funding admits execution, only the selected operator can claim, and its offer must declare the isolated backend.
5. Accepted work becomes settlement_pending. Results and a result digest are persisted before any chain action. The operator has not yet been marked paid.
6. A separate relayer signs settle(jobKey,resultDigest). The contract pays only the operator fixed at funding and permits one transition. Retrying or restarting the relayer cannot transfer USDC twice. Keys stay outside Rust coordinator/worker processes.
7. Rust re-reads confirmed contract state, verifies the result digest, and changes the job to succeeded with a USDC receipt, amount, payee and confirmed block hash. On-chain contract state is the payment authority; an HTTP acknowledgement is not proof of payment.

Cancellation produces refund_pending. The relay refunds funded escrow or permanently voids an unfunded quote, preventing a later funding race. After escrow expiry the relayer refunds automatically; anyone can also trigger a refund to the original buyer if the coordinator disappears. The job execution deadline is ten minutes; escrow expiry is one hour. Missing previously confirmed funding quarantines work for reconciliation. One failed relay item does not starve other payments.

The coordinator attests completion and can refund early. Embeddings have shape validation and worker-reported runtime evidence, not a cryptographic proof of useful computation. USDC issuer controls still apply. This is a centralized trusted-pilot settlement authority, not a trustless compute marketplace.

## Local setup

Requirements: Linux or WSL2, cgroup v2, Docker Engine with BuildKit and NVIDIA Container Toolkit, Rust/C toolchain, Node 22+, and the reviewed model cache. See official [Docker GPU access](https://docs.docker.com/engine/containers/resource_constraints/) and [NVIDIA toolkit installation](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html).

On this machine Docker and the NVIDIA toolkit were installed in WSL Ubuntu. For WSL without systemd, start scripts/start-container-engine.sh as root in its own terminal. This deliberately places the daemon in a non-root cgroup so Docker detects swap accounting; see [the upstream root-cgroup issue](https://github.com/moby/moby/issues/51480). It does not disable host swap or restart the user's gateway.

Run scripts/setup-isolation.sh in WSL. It builds the runner, creates an immutable image from the pinned upstream digest, writes .local/worker-isolated.json, and probes GPU readiness. A successful probe must report docker_v1 and gpu_observed=true. Setup never silently accepts mutable image tags.

Install settlement tools with `npm ci --prefix settlement`. Run `npm test --prefix settlement` for contract tests. The relayer is `node settlement/relay.mjs /absolute/coordinator.json`, with SOAPBOX_URL, SOAPBOX_ADMIN_TOKEN and SOAPBOX_SETTLER_PRIVATE_KEY in its private environment. For the testnet pilot, the integrated harness uses CDP-held signing accounts instead. Do not expose a private key through the coordinator configuration or desktop renderer.

## Full Bitterbot validation

From the sibling bitterbot-desktop repository, set SOAPBOX_REPO to this repository and SOAPBOX_BINARY to its Linux executable. SOAPBOX_SANDBOX_CONFIG defaults to .local/worker-isolated.json. Run `node --import tsx scripts/soapbox-payment-smoke.ts --gpu` for the complete gateway + real GPU + local EVM flow.

For Base Sepolia, first run scripts/soapbox-testnet-provision.ts with SOAPBOX_TESTNET_ACCOUNTS pointing to this repo's .local/testnet-accounts.json, using the existing CDP environment. It provisions separate test accounts and requests faucet assets. Then add --testnet to the payment smoke command. The live mainnet configuration is not edited. CDP currently rejects contract-creation transactions, so the harness deploys escrow using an ephemeral, faucet-funded test-only signer whose key stays in memory; actual buyer funding and settlement still use CDP. The funded job goes through the actual soapbox.payment.fund gateway handler and Bitterbot wallet service. The pilot uses 0.001 test USDC per input: a 33-input batch pays 0.033 test USDC.

The harness starts a separate authenticated gateway, coordinator and worker; submits private, collective and paid jobs; checks GPU evidence; measures operator token balances; restarts reconciliation to test idempotency; tests funded refunds and unfunded voids; and cleans up owned processes. SOAPBOX_REPORT selects the JSON evidence path. Testnet transaction hashes can be inspected on [Base Sepolia Explorer](https://sepolia.basescan.org/).

Validation evidence and limitations are recorded in [PHASE-2-PAYMENT-VALIDATION.md](PHASE-2-PAYMENT-VALIDATION.md). The contract source is contracts/SoapboxEscrow.sol. Solidity/compiler/dependency versions are pinned by settlement/package-lock.json. Rust dependency versions are pinned by Cargo.lock. See Circle's [official USDC addresses](https://developers.circle.com/stablecoins/usdc-contract-addresses) for the Base Sepolia token pin.
