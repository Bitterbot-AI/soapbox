# Bitterbot desktop integration source

The patch contains the Compute plugin, buyer/operator UI, wallet isolation, installer builder and tests. Apply it to Bitterbot desktop commit `df38874f18912865e299a657e50a874be1cb334a` with `git apply --index bitterbot-desktop.patch`. It is maintained in the desktop repository; this snapshot makes the private prerelease reproducible.

Build the desktop overlay against the exact installed release source with `node scripts/build-soapbox-integration.mjs /path/to/release-source`. This package targets Bitterbot 1.8.0 (`619ca967`, tag `v1.8.0`). Dependencies must be installed in both source checkouts. Then use `distribution/package.mjs` in Soapbox with the resulting `dist/soapbox-integration`, the release Rust binary, an output directory and the integration commit hash.

## Desktop setup preview

The integration snapshot at desktop commit `0c9485fdce2ab73aa33d6d7f7c2461a4eed54d22` supplies **Compute → Set up buyer or operator** and preparation of the included, checksum-pinned GPU runtime. The private **v0.3.0-rc.2** release packages this UI/plugin with click-to-open Windows/WSL and Ubuntu launchers, prerequisite checks and guarded rollback. See [the installer guide](../docs/TEST-RELEASE.md).

Buyers initialize a separate Base Sepolia wallet; operators import scoped enrollment and prepare the bundled runtime without a second download. Admin authorization, wallet separation, approved-workload isolation and per-job funding remain enforced. The Rust execution runtime remains the approved rc1 build; all payments remain testnet-only. This is a private add-on for Bitterbot 1.8.0, not a new stable Bitterbot release.
