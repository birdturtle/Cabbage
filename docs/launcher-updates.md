# Cabbage launcher update plan

Audit date: 2026-10-13. This is the implementation plan, not a working self-update installer.

## Current implementation

The release checker uses https://api.github.com/repos/birdturtle/Cabbage/releases. Both channels select the highest eligible semantic version from that repository. Stable excludes drafts, GitHub prereleases and tags with semantic prerelease suffixes. Beta includes prereleases; a stable version supersedes its matching RC. Older versions are never offered as upgrades. Metadata requests have a timeout and size limit.

The GitHub repository is public. At audit time it has no published releases. Cabbage's Cargo, Tauri and npm versions are all 0.1.0. The application ID is com.birdturtle.cabbage and the protected account service is com.birdturtle.cabbage.jagex.v1. Remaining HapticScape mentions are source attribution, screenshot origin and legacy-settings rejection tests. No runtime HapticScape update endpoint or suite manifest is configured.

Updates currently checks versions and displays notes. There is no Install update command, updater plugin/public key, signed launcher update artifact, or release packaging workflow. Tauri bundling is disabled; CI runs tests only. Development previews must remain outside installed-launcher update paths.

## Package strategy

Use Tauri v2's signed updater with a Cabbage-specific key. Windows starts with one standalone EXE offering per-user installation or portable use. Installed copies live under LocalAppData/cabbage/launcher and have a Start Menu shortcut and a user-scoped Installed Apps entry. Linux starts with an AppImage, with an install helper for desktop integration if needed. Tauri supports AppImage installation, but its Windows installer expects NSIS/MSI artifacts. The standalone Windows EXE will need verified download plus a separate replacement helper, or an internal installer that targets the same installation. Do not call the updater plugin installer on a raw EXE. Development executables and system-owned packages remain distinct. Tauri requires update signatures. Its updater key is separate from optional Windows Authenticode signing.

The installed launcher keeps its existing application ID, vault, preferences and client directories. The launcher package contains Cabbage and its small helper, not RuneLite, OSRS, RS3 or Wine. Game-client downloads and launcher updates remain separate.

## Game host requirement

RS3 now runs a separate cabbage-game-host executable with no GUI or account-vault dependencies. The launcher build compiles and embeds the platform helper. Before launch, Cabbage installs its exact bytes under the RS3 support directory named by the helper SHA256. Existing copies are verified and never overwritten; active versions are retained. The GUI executable and a future AppImage mount are no longer needed by the running supervisor.

The helper retains Linux FIFO IPC and Windows job tracking. It is embedded in the launcher so future package signatures cover its bytes. The helper keeps credentials in the child environment, preserves game lifetime tracking, and does not receive account-vault access. Test Close after Play and updates with an active host in actual installed packages on both platforms.

## Release selection and signing

Each Cabbage release should contain a cabbage-updater.json manifest with that exact release version and supported platform artifacts. Select the release through the existing channel policy, then fetch its manifest from that exact tag under birdturtle/Cabbage. Avoid a mutable beta tag that can point below a newer stable release. Require manifest version/tag agreement, a supported OS/architecture and HTTPS artifact URLs belonging to that Cabbage release. Artifact signatures must verify against Cabbage's embedded public key before installation. HapticScape's keys and manifests are not reused.

Release CI must check that the tag version matches Cargo, Tauri and npm, run tests, build Windows/Linux artifacts, perform installed-package smoke checks, sign the artifacts, and assemble the manifest only after all required platforms succeed. Store the private key in Cabbage's release secrets and keep an independent backup. Do not put private keys in source control or logs. Verify that an unsigned or incorrectly signed release cannot become an installable update.

## Install behavior and recovery

The Updates page will offer Install update only for a newer, supported release with a valid Cabbage manifest. Download progress, errors and retry stay visible. Downloads may be staged while a game runs. Applying an update must wait until client preparation, account writes and browser authorization are idle. Game processes and active helpers remain running. Restart launches the updated Cabbage installation rather than a development preview.

Use an explicit installer-written installation mode. AppImage and windowsExe installations receive their corresponding self-update paths. Development previews only check releases. System-package installations use their package manager. Do not infer package ownership from an absent marker, which previously caused incorrect update behavior in the parent project.

Keep the current launcher intact through download and signature verification. Retain a verified previous artifact for recovery and test actual installer failure and restart behavior. The updater plugin is not a substitute for an installed-package rollback test. Automatic post-startup rollback must not be claimed until an independent recovery mechanism is implemented and tested. Recovery must not clear account storage, preferences or game caches.

## Implementation order

1. Standalone versioned game host implemented; live Windows and installed-package acceptance remains.
2. Linux AppImage and Windows standalone EXE packaging with installation identity/mode are implemented; Windows CI and installed-package acceptance must pass.
3. Configure a Cabbage signing key and release CI.
4. Add the restricted Rust Install update command, progress/retry UI and restart handling.
5. Test upgrade between signed RC packages, matching RC-to-stable selection, malformed metadata, wrong signatures/targets, network interruption, locked files, failed restart, running games and preserved accounts.
6. Publish the release candidate after those checks pass.

## Primary reference

Tauri updater documentation: https://v2.tauri.app/plugin/updater/
