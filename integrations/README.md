# Bitterbot desktop integration source

The patch contains the Compute plugin, buyer/operator UI, wallet isolation, installer builder and tests. Apply it to Bitterbot desktop commit `df38874f18912865e299a657e50a874be1cb334a` with `git apply --index bitterbot-desktop.patch`. It is maintained in the desktop repository; this snapshot makes the private prerelease reproducible.

Build the desktop overlay against the exact installed release source with `node scripts/build-soapbox-integration.mjs /path/to/release-source`. This package targets Bitterbot 1.8.0 (`619ca967`, tag `v1.8.0`). Dependencies must be installed in both source checkouts. Then use `distribution/package.mjs` in Soapbox with the resulting `dist/soapbox-integration`, the release Rust binary, an output directory and the integration commit hash.

## Desktop setup preview

The integration snapshot at desktop commit `8310273f57e953c0725251af33a7f5f5768913b2` adds **Compute → Set up buyer or operator**. Buyers can create a separate Base Sepolia wallet and import enrollment. Operators can import enrollment, upload the checksum-pinned runtime from the published release, prepare the GPU and activate with a resource allowance. All setup endpoints require an authenticated gateway admin. Mainnet wallet settings are preserved.

A fresh-profile RTX 4080 operator completed job `0c3a70ce-bc8f-4f91-8f99-a07b44bb237e` and received 0.002 test USDC after setup. Two 384-dimensional vectors, wallet separation and duplicate-settlement prevention were verified on two gateway profiles on one physical host. Physical two-machine validation is still pending. See the desktop patch’s `docs/network/soapbox.md` for the GUI guide and `soapbox-gui-validation.json` for evidence.

The immutable v0.3.0-rc.1 archive remains the approved runtime input. Its bundled desktop UI predates this wizard; use a desktop build containing this updated integration snapshot. This snapshot does not publish or replace a stable Bitterbot release.
