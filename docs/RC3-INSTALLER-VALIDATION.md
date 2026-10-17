# Soapbox v0.3.0-rc.3 installer validation

Validated on 2026-10-17 for the private Base Sepolia pilot. Compute remains visible
and enabled by default; enrollment, operator activation and each payment are explicit
user actions. See [review disposition](RC3-REVIEW-DISPOSITION.md).

## Provenance and artifact integrity

The tested installer packages were built from Soapbox source commit
`93998123152c47245d872dd10c69b7861d289b29` and Bitterbot desktop integration commit
`6f4afe9f3f487981230670103343d925432d8b5c`, targeting the installed Bitterbot 1.8.0.
The Rust release binary was built from `a5c5590fddf3606b250394e79f749786cc9c2c03`;
the subsequent packaging commit did not change Rust execution code. Later validation
documentation does not change the tested executables or archives.

All 403 files in the release manifest passed SHA-256 verification. ZIP integrity
passed. The runtime archive's binary matches the top-level packaged binary exactly.
Source/history and the expanded package were scanned against 16 known local
credentials and credential patterns: 654 objects checked, zero findings. This
targeted scan is not a substitute for an independent security assessment.

| Artifact | SHA-256 |
| --- | --- |
| soapbox-0.3.0-rc.3-linux-x64.tar.gz | `be540cffa974faa3cbdef97b399c8a8022fcef9f85ebd178ce96af4d6496aa0c` |
| soapbox-0.3.0-rc.3-windows-wsl.zip | `c29b04a5dca8b89e9899880af61e3791bef7af0378375b805f3adb9be6991a7f` |
| soapbox-runtime-0.3.0-rc.3-linux-x64.tar.gz | `2bcabeeb13ca212ecaf9af0e2860b30d53f3a4b13f1956fb962bf0ebf2936190` |

## Automated checks

- GitHub [CI run 39064609009](https://github.com/Bitterbot-AI/soapbox/actions/runs/39064609009)
  passed all five jobs for the packaged source: Rust formatting, Clippy, tests and
  launcher compilation on Linux, Windows and macOS; settlement tests; distribution tests.
- Nine installer/enrollment tests passed, including tampered assets, mainnet refusal,
  local API authentication, prerequisite failures, backup recovery and guarded rollback.
- Eight setup/runtime tests passed with the rc3 runtime pin. Registration/scope tests
  loaded the plugin against the actual core handler registry with unrelated handlers
  stubbed; buyer/operator registration and role boundaries passed.
- Funding and wallet tests covered quote approval, changed quotes, dedicated account
  identity, address collisions, spending caps, pinned chain/escrow/token/bytecode,
  approve-before-fund ordering, reverted approvals and bounded receipt waits. RPC
  calls were mocked for deterministic rejection/order tests; these are not chain receipts.
- Compute view tests, static payment-path checks, changed-file lint/formatting,
  full desktop TypeScript check and the packaged UI/plugin build passed.
- Linux and Windows launchers are byte-for-byte identical to the launchers already
  compiled and exercised in [rc2 validation](RC2-INSTALLER-VALIDATION.md), including
  the Windows launch from an extracted ZIP path containing spaces. That native launch
  was not repeated for rc3; the rc3 packaged local installer was exercised below.

## Live clean-profile installation

The test used a new application profile on the existing Ubuntu 24.04 WSL2 host,
with installed Bitterbot 1.8.0, Docker/NVIDIA support and the RTX 4080. It started
with no Compute configuration or test wallet. Main-app CDP credentials were not copied.

1. Opened the local installer from the actual rc3 package. Buyer prerequisites passed,
   and the UI installed the wizard without enrollment or payment.
2. Started the isolated gateway and opened Compute from the normal sidebar. The buyer
   wizard loaded with empty CDP credential fields and a disabled wallet creation button.
3. Operator prerequisites passed: Docker access, cgroup v2, swap enforcement, GPU and
   NVIDIA container runtime.
4. The new import check rejected an old scoped enrollment with a private RPC endpoint.
   Only the test copy's `rpc_url` was changed to `https://sepolia.base.org`; its existing
   identity, token and permissions were retained. The setup RPC then accepted it.
   This exercised the import backend; the native file picker was not automated.
5. Clicked **Prepare GPU runtime**. The bundled rc3 archive passed its exact digest;
   the fixed container built, the pinned model downloaded, and the real GPU/isolation
   probe completed at 100% with **GPU runtime verified**.
6. Activated the scoped operator through the UI and verified the reconnected dashboard
   showed the operator online and the GPU probe passed on Base Sepolia.
7. Used the installer's rollback action after activation. The original profile
   configuration was restored exactly. The profile's wallet marker and the main app's
   wallet configuration/file hash were unchanged. Wallets, backups and models were retained.
8. Closed the temporary installer and stopped only the isolated test gateway.

## Limits and next acceptance test

This was a clean application-profile test, not a fresh OS or driver installation.
No new buyer wallet, paid job or blockchain payment was created during installer
validation. Existing coordinator history on the operator dashboard is not evidence
of a new payment. Physical two-machine testing still needs the second tester.

The unsigned add-on supports Bitterbot 1.8.0 on Ubuntu 24.04 x64 / WSL2, with the
default WSL distribution used on Windows. Operators must install hardware/container
prerequisites separately. Signed builds, broader compatibility, untrusted operators,
public multi-tenant deployment and mainnet remain outside this release.
