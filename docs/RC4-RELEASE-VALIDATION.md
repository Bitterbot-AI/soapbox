# Soapbox v0.3.0-rc.4 release validation

Validated on 2026-10-17. This private payment-fix release keeps Compute visible and
enabled by default, with Base Sepolia test USDC and a dedicated Compute wallet.
Enrollment, operator activation and quote approval remain explicit user actions.

## Fix and provenance

The published rc3 plugin contained an extra hexadecimal character in the SHA-256
pin for the approved escrow contract. Funding correctly refused the mismatched pin.
rc4 ships the verified 64-character pin and a regression test using the actual
release policy rather than a mocked policy.

- Soapbox packaging source: `766b71195ef104d09d1dbaf35134f1e2f766ef91`.
- Desktop integration source: `268c765e84b233312e3924c427dbfba46c5ba57c`.
- Compatible installed desktop: Bitterbot 1.8.0 (`619ca967`, tag `v1.8.0`).
- Rust runtime remains rc3, source `a5c5590fddf3606b250394e79f749786cc9c2c03`.
- Later validation documentation does not alter the tested archives.

The private integration patch applies cleanly to its documented desktop base,
`df38874f18912865e299a657e50a874be1cb334a`. Both rebuilt JavaScript bundles were
byte-compared with rc3 after normalizing only the corrected hash and rc4 release URL:
there were no other bundle changes. All 394 manifest entries for UI, launchers,
installer implementation, binary and runtime are unchanged from rc3.

## Artifact integrity

All 403 release manifest files passed SHA-256 verification, the Windows ZIP passed
integrity and path checks, and all archive checksums matched. The package contains
no wallet/configuration files, enrollment credentials, databases or logs. A targeted
scan of the tracked source and expanded package against known local credentials
found no matches; it does not replace independent security review.

| Artifact | SHA-256 |
| --- | --- |
| soapbox-0.3.0-rc.4-linux-x64.tar.gz | `4eca44e2cef4b5eb57d8cdd2f264aa4ed350c3edc605c275305f2c8f728bcdef` |
| soapbox-0.3.0-rc.4-windows-wsl.zip | `37f6f29afd3204011d2bd576fdc01cb222b75e0cbb5e0891a5d64295a6e36ed1` |
| soapbox-runtime-0.3.0-rc.3-linux-x64.tar.gz | `2bcabeeb13ca212ecaf9af0e2860b30d53f3a4b13f1956fb962bf0ebf2936190` |

## Checks performed for rc4

- 26 payment, wallet and setup tests across seven files passed; nine gateway
  registration/scope tests across two files passed.
- Nine distribution/installer tests passed, including prerequisite rejection,
  mainnet manifest refusal, tampered assets, installer authentication and rollback.
- Changed desktop files passed the repository's type-aware lint and formatting hook.
- The desktop plugin build targeting Node 22 passed; the UI is unchanged from rc3.
- The packaged installer was started with three isolated application profiles:
  fresh buyer, fresh operator, and rc3 upgraded with existing participant settings.
  Each used the installed Bitterbot 1.8.0 application and actual host prerequisite
  checks; Docker/NVIDIA/cgroup v2/swap checks passed for the operator.
- Installation and rollback ran through the packaged local HTTP installer API.
  All three profiles enabled Compute, installed the rc4 plugin and preserved
  mainnet configuration plus distinct mainnet/test-wallet markers. Upgrade retained
  participant settings. Rollback restored each original configuration exactly.
- The running main application's mainnet wallet configuration and both wallet
  identity files were checked against the prior validation snapshot and were unchanged.

## Payment verification

The packaged payment function was exercised against the live Base Sepolia RPC using
a temporary bundle copy with test-only exports. It verified chain 84532, the pinned
escrow bytecode, its USDC token and an empty random escrow key. A sentinel stopped
execution at the spend-reservation boundary: no wallet credentials were loaded,
no signing function was invoked and no transaction was broadcast. The release
bundle and installed main application were not modified by this probe.

The preceding main-app UI validation used this same payment-pin correction:

- Job: `5aa031e5-eb00-4af7-a9aa-96cb3919dadf`, succeeded.
- Workload: two GPU embeddings, 384 dimensions, NVIDIA GeForce RTX 4080.
- Buyer debit and operator credit: 0.002000 test USDC each.
- Settlement: `0x9bbc28df690e3671e7b560091657a447542f1ad55558effe0919de25dd05c51b`,
  successful receipt with 49 confirmations at evidence capture.
- Mainnet wallet configuration and wallet identity remained unchanged.

The UI test was rc3 plus the local pin hotfix, before rc4 packaging. It is carried
forward as payment evidence, not claimed as a second rc4 on-chain payment. rc4
packaging did not spend funds or replace/restart the running main application.

## Remaining acceptance boundaries

Clean profiles were tested on the existing Ubuntu 24.04 WSL2 host, not a new OS or
driver installation. The unchanged Windows launcher and UI retain the rc3/rc2
validation recorded in [rc3 validation](RC3-INSTALLER-VALIDATION.md); no new native
Windows launch or full UI click-through was claimed for rc4.

The paid GPU test used one physical machine. Two-machine acceptance, signed
installers, untrusted operators, broader compatibility and public multi-tenant
deployment remain separate work. All payments remain testnet-only. PLAN-57 is
unchanged pending review.
