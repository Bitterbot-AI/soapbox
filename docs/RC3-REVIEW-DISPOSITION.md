# Review disposition for the private rc3 candidate

The maintainer chose to keep Compute enabled and visible by default and retain the
private Base Sepolia pilot. The review was assessed against the current source and
the packaged Bitterbot 1.8.0 integration. This is a targeted correction pass, not a
claim that every public-marketplace or mainnet recommendation is complete.

## Included corrections

- **A1:** typed, static public API errors; generic correlated responses for unknown
  errors; fixed payment-observation error responses; transport URL removal; public
  RPC enrollment exports and validation. Missing resources and unauthorized access
  now receive 404 and 403 where covered. Regression tests exercise a failing RPC
  URL containing private path/query values and malformed stored job data.
- **C0:** the plugin is the sole owner of Compute gateway handlers and services.
  Registry integration tests load buyer and operator modes against the actual core
  handler table and verify no registration collisions and preserved role behavior.
  Unrelated gateway handlers are stubbed in these focused tests.
- **C3:** new Compute wallets use a dedicated CDP account name. Main/Compute address
  collisions and unexpected CDP identity changes are rejected. Existing separate
  legacy test wallets retain their identity; a legacy account is blocked if the
  main wallet uses Base Sepolia. No funds or identities are migrated automatically.
- **C2, targeted private-pilot controls:** admin-only funding/refresh, approval of
  the exact displayed quote, an immutable per-wallet approval snapshot, a private
  approval record, the existing spending caps, and an escrow address/bytecode hash
  pinned by the release. Enrollment cannot choose another contract. Static payment
  checks now detect Compute funding and raw CDP signing outside reviewed entrypoints.
- **C6/C9, targeted controls:** bounded RPC and receipt waits; tests cover chain,
  bytecode and token rejection before signing, reserve/approve/confirm/fund ordering,
  reverted approvals, timeouts, quote changes and wallet identity mismatches.
- **C10/A11, installer work already completed:** bundled runtime installation and
  preparation are wired and tested. Configuration changes preserve unrelated fields;
  guarded rollback works after operator activation and retains wallet files.

## Retained product choices and remaining milestones

- Compute stays in the main sidebar and enabled by default. Enrollment, worker
  activation and each payment still require deliberate user actions.
- The release remains an unsigned private add-on for the tested Bitterbot 1.8.0
  version on Ubuntu 24.04 x64/WSL2. Signed CI builds, broader version compatibility
  and a plugin-contributed UI instead of the version-specific overlay are future work.
- A wallet-free public test-credit enrollment flow was not substituted for the
  requested private Sepolia release.
- General AP2 mandate enforcement on newer desktop main, contract grace/recovery
  changes, relayer outbox and retry handling, queue fairness, broader service
  hardening, untrusted-operator isolation and public rollout remain separate work.
  The private pilot's explicit-click approval gate is not a replacement for that
  future AP2 integration or an external contract/security review.
- Physical two-machine testing still requires the second tester. Installation
  validation uses a fresh application profile on an existing supported host.

The earlier rc2 build remains an unpublished draft. The rc3 runtime is rebuilt with
the API corrections and its digest is pinned by the desktop package. Nothing in
this pass enables mainnet settlement or moves main-wallet funds.
