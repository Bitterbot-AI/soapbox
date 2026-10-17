# Soapbox v0.3.0-rc.2 installer validation

Validated on 2026-10-17. This unsigned Base Sepolia candidate remains an unpublished draft, superseded by [rc3](RC3-INSTALLER-VALIDATION.md).

## Scope and provenance

The tested candidate archives were built from Soapbox commit
`befa187380969d0dcfe711676fbd8d8b4633cd48` and Bitterbot desktop commit
`7d08b9fd242d2bb19fa499b55bfa049c595bdc82`. The later desktop documentation
commit `0c9485fdce2ab73aa33d6d7f7c2461a4eed54d22` is included in the integration
source snapshot. No executable code changed after packaging this candidate.

The bundled GPU runtime remains the previously approved v0.3.0-rc.1 archive,
verified by its fixed SHA-256 digest. Installer and wizard changes are rc.2.

| Artifact | SHA-256 |
| --- | --- |
| soapbox-0.3.0-rc.2-linux-x64.tar.gz | `d3e501498c6cdb9308e3d7ecbd5e78ad49d3f2d72bf0ba31c12e314d0b74f8d8` |
| soapbox-0.3.0-rc.2-windows-wsl.zip | `612ba70c2edaeac1765d1d8cb6d81fc5ec726587f82ef0ba821d92111bb682ac` |

## Automated checks

- 9 installer/enrollment tests passed, including no-enrollment clean installation,
  prerequisite failures, tampered assets, mainnet rejection, authenticated local
  API checks, backup recovery, pending-job rollback refusal and preservation of
  later unrelated settings.
- 8 desktop setup/runtime backend tests and 12 Compute UI tests passed.
- Full desktop TypeScript check passed. Changed TypeScript files passed lint and
  formatting checks. Integration UI/plugin build passed.
- Linux and Windows launchers compiled with Rust warnings denied. Windows PE
  inspection confirmed the GUI subsystem and normal system DLL dependencies.
- ZIP integrity and safe archive paths were checked; all 402 release manifest
  files were verified. The top-level Soapbox binary matches the pinned runtime.
- Source/history and expanded package were scanned against known local secrets
  and credential patterns before publication; no findings.

## Live installation, GPU preparation and rollback

The installation used a newly created Bitterbot profile with no prior Compute
configuration or test wallet, on the existing Ubuntu 24.04 WSL2 host. Installed
Bitterbot 1.8.0 software was reused; user data was not copied. The host already had
Docker, NVIDIA support and an RTX 4080.

1. Opened the local installer from the actual packaged release. Buyer prerequisite
   checks passed, and the UI installed the wizard without enrollment or payment.
2. Restarted the isolated gateway normally. The buyer wizard loaded with empty CDP
   credential fields. Main-wallet credentials were not imported or reused.
3. Operator prerequisite checks passed for accessible Docker, cgroup v2, swap-limit
   enforcement, NVIDIA GPU and NVIDIA container runtime.
4. Imported an existing scoped test operator enrollment through the gateway setup
   RPC. This step reused existing authorization; it did not create an enrollment.
5. Clicked **Prepare GPU runtime** in the packaged desktop UI. The bundled archive
   passed its checksum, the container image built, the approved model downloaded,
   and the real GPU/isolation probe completed successfully.
6. Activated the operator through the desktop UI. The dashboard reconnected and
   showed the enrolled operator online, Base Sepolia payout network and GPU probe
   passed. Existing coordinator job history was displayed; no new paid job was
   submitted during this installer test.
7. Used **Roll back this installation** in the installer UI after activation.
   The original profile configuration was restored exactly. The profile wallet
   marker was unchanged, and the main app's wallet configuration and wallet-file
   hash were unchanged. Wallet data, backups and models were retained.
8. Closed the temporary installer and stopped only the isolated test gateway.

The packaged Windows executable was also launched from an extracted ZIP in a
directory containing spaces. It started the installer through the default WSL
distribution, served the installer page on a loopback port and successfully
invoked the Windows browser opener. After the test server was stopped, the
launcher exited with code 0. The test needed the normal host context because the
coding shell's sandbox cannot access WSL. No system security settings changed.

## Limits of this validation

- This was a fresh application profile on an existing supported host, not a fresh
  Windows/Ubuntu OS installation or driver installation.
- Buyer setup rendering and backend/UI wallet tests passed; this installer test
  did not create or fund another buyer wallet or submit a new blockchain payment.
- Physical two-machine/WAN testing still requires a second tester. This test did
  not validate public multi-tenant operation, macOS installation or mainnet.
- The installer requires existing Bitterbot 1.8.0 on Ubuntu 24.04 x64, with WSL2
  supported. Windows uses the default WSL distribution. Operator dependencies
  must be installed separately. This is an unsigned preview, not a signed
  production installer.
