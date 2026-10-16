# Validation record

**Current GPU evidence:** [Phase 2 native GPU validation](PHASE-2-VALIDATION.md).
The record below describes Phase 1 only; its no-physical-GPU statement is historical.

Date: 2026-10-16. Source state: initial Soapbox v0.1 implementation.

## Passed

- Rust formatting: cargo fmt --all.
- Rust static analysis: cargo clippy --workspace --all-targets --locked -- -D warnings.
- Rust tests: **21 passed** (3 worker/embedding-adapter tests, 15 lifecycle/accounting/isolation tests,
  3 network/authentication/multi-process tests).
- Standalone two-worker CLI demo: all three modes succeeded; only the paid job consumed credits;
  replayed submissions returned the original job; artifact contents round-tripped.
- Bitterbot integration tests: **10 passed** across client, gateway routing, and RPC authorization.
- Desktop component tests: **2 passed** for disabled setup and authorized private submission.
- Gateway TypeScript check: tsgo --noEmit.
- Desktop TypeScript check: tsc --noEmit -p desktop/tsconfig.json.
- Targeted type-aware lint: zero warnings/errors on new integration code.
- Desktop style/file-size guards: passed.
- Desktop production build: passed (4503 modules; existing large-chunk warnings remain).
- Managed lifecycle smoke test: real gateway handlers → Rust coordinator → worker → receipt.
  All modes completed; pause blocked new work; cancellation refunded its reservation;
  temporary artifacts round-tripped; the gateway stopped its own subprocesses.
- Git whitespace check for the Bitterbot integration: passed.

The main execution environment was WSL Ubuntu with the installed Rust toolchain and Node 22.22.1.
Desktop TypeScript checks also ran with the available Windows Node runtime.

## Boundaries of this evidence

- Embedding API behavior and GPU-residency signals were tested against a local mock Ollama server.
  No physical GPU or real embedding model was exercised.
- No real payment provider, USDC transfer, cash escrow, withdrawal or partner API was exercised.
- Storage tests cover temporary, owner-scoped coordinator artifacts, not replicated durability.
- Multi-worker tests use independent processes on one machine; they are not WAN or fleet load tests.
- The full legacy Java distribution was neither rebuilt nor executed.
- Cross-platform CI is configured; it has not run remotely.
- No external deployment or GitHub publication was performed.
- The full Bitterbot repository test suite was not run; focused tests, full type checks and
  the renderer build covered the integration.

## Native Windows validation

- Cross-compiled x86_64-pc-windows-gnu using the installed Rust target and MinGW toolchain.
- Ran soapbox.exe --version successfully on Windows.
- Ran the complete native Windows coordinator/two-worker demo with Windows Node: passed.
- All three modes succeeded; available balance was 98 of 100 test credits with zero reserved;
  artifacts round-tripped and all demo processes were stopped.
- Native debug executable: .local/bin/soapbox.exe (ignored by Git).
- This is an unsigned development binary, not a production installer.
