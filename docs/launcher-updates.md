# Launcher updates

The checker reads releases from `birdturtle/Cabbage`. Stable excludes drafts and prereleases. Beta includes prereleases, with stable versions taking priority over matching RCs. Older versions are never installed.

## Signed release downloads

Each release contains `cabbage-updater.jwt`. Its RS256 signature covers the exact version, platform, GitHub asset URL, byte count and SHA256 of both launcher downloads. The launcher embeds Cabbage's public key in `src-tauri/updater-public.pem`. The release workflow uses the repository secret `CABBAGE_UPDATE_PRIVATE_KEY`; signing fails if it does not match that public key. The private key is never included in source or release assets.

The signing key backup is held separately from the repository. Preserve it across developer machine changes. Replacing the public key without a transition prevents older installed launchers from accepting future updates.

## Installation

Updates support the standalone Windows executable and Linux AppImage, both installed and portable. The running artifact determines the replacement target. Development previews can check releases but cannot install updates. Package-owned builds must add explicit package-manager behavior before distribution; the absence of a marker does not imply package ownership.

The signed artifact is downloaded to a unique sibling of the current launcher. Size and hash are checked while downloading. Only GitHub release HTTPS redirects are accepted. Interrupted downloads are removed and can be retried. Account writes, browser authorization and client preparation must finish before an update applies.

The embedded standalone helper rechecks the staged hash and waits for the launcher to exit. It retains the previous artifact, activates the new one and starts it without inherited AppImage mount paths or Jagex credentials. The updated UI acknowledges startup only after loading launcher status. Windows installed copies also refresh their Installed Apps version. If startup fails or does not acknowledge within 90 seconds, the worker restores and reopens the previous launcher. Recovery errors are shown on the next startup. The previous artifact remains beside a successful update as a `.previous` backup.

No account storage, preferences, game cache or running game process is replaced. The game supervisor remains independent of the GUI.

## Acceptance

Automated coverage includes signature policy, matching release/platform/URL, interrupted and corrupt downloads, replacement, successful restart and failed-start rollback. Both OS runners execute helper tests using native fixture executables.

RC2 introduces installation; RC1 requires a manual download. RC2-to-RC3 must be tested with actual release assets on Windows and Linux before stable publication. Test installed and portable copies, preserved accounts/preferences, an active game and RC-to-stable selection. GUI acceptance is separate from the fixture restart tests.
