# Bitterbot Desktop integration

The rc5 integration is maintained in the public
[Bitterbot Desktop repository](https://github.com/Bitterbot-AI/bitterbot-desktop).
Its exact source revision is recorded as `desktop_commit` in each package's `release.json`.
The included `bitterbot-desktop.patch` is a source snapshot against desktop commit
`df38874f18912865e299a657e50a874be1cb334a`. Apply it with `git apply --index` to that revision,
or check out the recorded desktop commit directly.

## Build the matching application overlay

This release supports Bitterbot 1.8.0, whose source is tag `v1.8.0`, commit
`619ca967fe4d8a75142ec064ba00bb917efd3012`. Install dependencies in both the integration
checkout and the supported release checkout, including their desktop workspaces. Use
Node.js 22, Python 3, Git, GNU tar/gzip, and the repository's pinned Rust toolchain.

From the integration checkout, run:

```sh
node scripts/build-soapbox-p2p-release.mjs /path/to/bitterbot-1.8.0-source
```

The builder stages the supported release, applies the Compute changes, and builds matching
gateway, plugin and UI outputs into `dist/soapbox-integration`. It preserves the supported
release's existing non-Compute features instead of replacing them with the older integration
checkout. The build requires the fixed rc4 comparison commit `268c765e` in local Git history.

Build Soapbox with `cargo build --release --locked -p soapbox`. Build the desktop repository's
P2P sidecar with `cargo build --release --locked` in `orchestrator/`. The runtime archive contains
`bin/soapbox`, `containers/`, `LICENSE` and source-hash `provenance.json` under the directory
named by `distribution/runtime.json`. The package builder verifies the archive's approved
SHA-256. Rebuilt binaries may differ with compiler/environment changes; a changed archive
requires updating and reviewing both the runtime manifest and the desktop runtime pin.

Build the native launchers from `distribution/launcher.rs` with `rustc --edition 2021 -D warnings`;
the Windows build additionally uses `--target x86_64-pc-windows-gnu` and a MinGW linker.
Run `distribution/package.mjs` with `--desktop`, `--binary`, `--orchestrator`, `--output`,
`--desktop-commit`, `--runtime`, `--windows-launcher` and `--linux-launcher`.

## Operator and buyer behavior

**Compute → Set up buyer or operator → GPU operator → Enable this GPU operator** prepares
and verifies the isolated runtime, then activates a supervised local coordinator and worker.
Private and collective P2P work needs no wallet or enrollment file. Operators explicitly grant
peer IDs access to their pools/projects. Discovery and execution use the existing Bitterbot
P2P identity and transport.

The advanced coordinator-enrollment path retains the paid Base Sepolia pilot. Buyers use a
separate Compute wallet and approve each quote. Compute remains visible and enabled by
default. Paid P2P fan-out is not included. See [setup and rollback](../docs/TEST-RELEASE.md)
and [rc5 validation](../docs/RC5-RELEASE-VALIDATION.md) for the tested scope.
