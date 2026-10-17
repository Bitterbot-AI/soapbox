# Bitterbot desktop integration source

The patch contains the Compute plugin, buyer/operator UI, wallet isolation, installer builder and tests. Apply it to Bitterbot desktop commit `df38874f18912865e299a657e50a874be1cb334a` with `git apply --index bitterbot-desktop.patch`. It is maintained in the desktop repository; this snapshot makes the private prerelease reproducible.

Build the desktop overlay against the exact installed release source with `node scripts/build-soapbox-integration.mjs /path/to/release-source`. This package targets Bitterbot 1.8.0 (`619ca967`, tag `v1.8.0`). Dependencies must be installed in both source checkouts. Then use `distribution/package.mjs` in Soapbox with the resulting `dist/soapbox-integration`, the release Rust binary, an output directory and the integration commit hash.
