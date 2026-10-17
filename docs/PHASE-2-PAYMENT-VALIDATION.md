# Isolation and payment validation

Validated on 2026-10-16 (Toronto; evidence timestamps use UTC) using the RTX 4080 in WSL Ubuntu, Docker 29.1.3, NVIDIA Container Toolkit 1.20.1, Ollama 0.40.5, and the digest-pinned all-minilm model.

## Completed local checks

- Rust: 29 unit, lifecycle and network tests; strict Clippy passes.
- Real container tests: CPU SHA-256 output; non-root identity, no host filesystem/socket/secret access, no external networking, read-only model cache, seccomp, no privilege escalation, zero swap, PID limit, 64 MiB out-of-memory kill and container removal.
- Escrow and relayer on a local EVM: fixed-payee payout, unauthorized settlement rejection, duplicate rejection, insufficient-funds atomicity, early refund, permissionless expiry refund, automatic relayer expiry refund, unfunded cancellation and retry acknowledgement.
- Bitterbot: 13 original integration/payment checks plus 12 wallet spending/reservation checks; six desktop tests including exact USDC quote and explicit funding action. TypeScript checks, focused lint and desktop guard scripts pass.
- Production desktop Vite and gateway entry bundle builds pass; existing large-bundle warnings remain. Windows GNU Soapbox executable built and its CLI startup verified; sandbox execution evidence remains Linux/WSL.
- Full authenticated gateway flow: private, collective and paid GPU batches, 33 rows of 384 dimensions per job, three bounded inference batches, full GPU residency reported. Local operator balance rose from 0 to 33,000 token micro-units.
- A simulated crash after confirmed payout and before coordinator acknowledgement recovered on a fresh relayer without paying twice. Funded cancellation refunded; unfunded cancellation permanently voided the quote.

Local evidence: [phase2-isolated-local.json](evidence/phase2-isolated-local.json). The local token is an explicitly labelled TestUSDC fixture; it is not public-chain evidence.

The worker deferred admission while host RAM was insufficient, then resumed. Cold WSL imports and model startup are slow under the current host load; this run is functional evidence, not an SLA or throughput benchmark.

## Scope

See [execution and payment design](PHASE-2-ISOLATION-PAYMENTS.md) for setup and reproduction. Container execution is validated on Linux/WSL; no new native Windows sandbox or packaged desktop installer is claimed. Public mainnet is disabled. The coordinator attests accepted work; the pilot does not prove useful computation cryptographically. RTX 4080 VRAM is not hardware-partitioned. Independent security/contract review and commercial SLA qualification remain required before a public paid fleet.

## Public Base Sepolia result

Completed 2026-10-17T00:42:42Z. The exact same authenticated Bitterbot WebSocket flow ran isolated private, collective and paid GPU jobs. The paid job processed 33 texts into 384-dimensional embeddings in three batches, with full model GPU residency, and completed on its first attempt.

- Buyer: `0x6206c0743e09D07e2F29673f23A2cBD0F86e0795`.
- Operator: `0xCD457D157357627e2048B4BE958D8b937C1C7af5`.
- Escrow: `0x85dbc619cd8f05e06a5fc231d04483b6aa7bff17`.
- [Buyer funding through Bitterbot wallet RPC](https://sepolia.basescan.org/tx/0x4fe0382f0f3f0e8ea1442c726fdb96106aad6d177644f7c3287092fc3aa606e9).
- [Operator payout](https://sepolia.basescan.org/tx/0x4d36c054324a4ad7a32e3454adee72b7b14facfcd096d607f512618e41ab1184).
- Operator balance increased from **0 to 0.033 test USDC**, exactly 33,000 micro-units of Circle's Base Sepolia token.
- [Funded cancellation refund](https://sepolia.basescan.org/tx/0x752746687461a1390ce9c73f57567b47191de58750ea80265b12e9d7c99b90cc).
- [Unfunded quote cancellation](https://sepolia.basescan.org/tx/0x2af57584fff46e14cdf04cfa8fb61db26a3a83ca13c285ccc3d9223619ee4ad9).
- Confirmed payout followed by simulated relayer failure, then fresh reconciliation and repeated polling: no second payout.

The paid receipt's result SHA-256 is `ba79ec5d6209f9b04e47093768bf0f97f693825def65271932b650f5d7120879`. Its confirmed block hash is `0xdb740ae5800e39b398e484ea87642886b038b50f78be9a1a14aef813387fd7c0`.

Full public evidence: [phase2-base-sepolia.json](evidence/phase2-base-sepolia.json). A [second-RPC verification](evidence/phase2-chain-verification.json) independently confirms the successful funding and payout receipts, exactly one 33,000-unit Transfer to the operator, the settled result digest, and zero USDC left in escrow. Lease credentials were omitted from the saved report. These are test assets with no monetary value. The existing Base mainnet wallet and running gateway were not reconfigured or restarted, and no mainnet transaction was sent.

For repeated runs, set `SOAPBOX_TESTNET_REUSE_REPORT` to an earlier testnet report to reuse its escrow. The harness checks the deployed bytecode pin, USDC token and coordinator signer before reuse. Run large builds separately from GPU validation on this 5 GiB WSL VM; the worker correctly defers when the requested RAM allowance is unavailable. The development wallet funding path uses CDP directly inside Bitterbot's wallet service, preserving spend caps without loading unrelated AgentKit integrations.