# Recovery testing

Checked 2026-10-13 on Linux. Production account data and installed games were not modified. Tests use local HTTP fixtures and isolated temporary directories, including paths with spaces.

## Completed automated checks

- Truncated HTTP bodies and oversized streams fail before acceptance.
- A RuneLite download interrupted mid-response can retry in fresh staging. The active record remains unchanged.
- Bad SHA256/CRC, chunk framing, signature and archive paths are rejected by existing adapter tests.
- Failed record activation preserves existing data and removes its temporary record. Concurrent readers only see complete installation records.
- Interrupted replacement restores old binaries before the active installation record. A committed replacement is kept. A missing recovery journal never triggers deletion of its backup.
- Repair and uninstall preserve saved accounts, game settings, Wine-prefix data and caches. Symlink targets outside managed artifacts are not deleted.
- Maintenance reservations block conflicting launches. Native and Wine paths are detected after the GUI exits.
- Expired OAuth/game sessions renew through local fixtures. A later network failure retains the rotated refresh token; the next attempt uses it and completes without losing character selection. Invalid grants retain the account and request sign-in.
- Stable filtering rejects RC tags even when GitHub's prerelease flag is wrong. Stable supersedes a matching RC with beta enabled; older releases do not become upgrades.

## Live artifact checks

The isolated RuneLite/Java and official OSRS repair, uninstall and reinstall tests passed using current upstream downloads. They retain sentinel game settings, Wine-prefix data and account data in their temporary folders. These checks do not start or sign into games. The RS3 repair/uninstall/reinstall cycle also passed using the actual Jagex client and pinned Ubuntu compatibility libraries.

## Fixes from this audit

The shared record writer previously moved current.json aside before activating the replacement, leaving a crash window without an active record. It now writes and syncs a unique temporary file and replaces current.json in one rename. Rust supports destination replacement on Unix and Windows. This addresses the application-level gap; it is not a claim of filesystem recovery from every power-loss or hardware failure.

The stable release path previously depended on GitHub's latest-release endpoint and prerelease flag. Both channels now use the version-sorted Cabbage release listing, with stable checking the semantic prerelease suffix too. Metadata body size and per-request timeout are explicit.

## Remaining release checks

Windows CI runs the non-network regression suite. Real Windows gameplay and installed-package recovery are still separate acceptance checks. Launcher self-update installation is not implemented; its signed upgrade, restart and recovery checks are prerequisites to release, documented in launcher-updates.md.
