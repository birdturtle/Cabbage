# Cabbage client expansion

## Goal and baseline

Launch RuneLite, the current official OSRS client, and RS3 from Cabbage on Windows and Linux using the selected Jagex account and character. Cabbage owns installation, updates, and launch preparation; users should not need to install another launcher to play.

On 2026-10-12 the user confirmed Jagex login and RuneLite launch on Linux after reboot. This is one successful live test, not proof that the earlier Cloudflare loop is resolved everywhere. Preserve the committed authentication changes at `3496ddd` and the working browser profile while expanding client support. Windows login and gameplay still require live acceptance.

## Target matrix

| Client | Windows | Linux |
| --- | --- | --- |
| RuneLite | Existing managed Java integration | Existing managed Java integration |
| Official OSRS | Current official Windows client | Investigate current official Linux delivery; otherwise official Windows client through managed Wine |
| RS3 | Current official Windows client | Investigate and validate official native Linux client and dependencies |

“Official OSRS” means the current native game client, not the legacy Java applet. Wine is a compatibility implementation if needed on Linux. Establish x86_64 acceptance first for official clients; expose other architectures only where upstream artifacts and runtime testing support them. Retain existing RuneLite platform support.

## Phase 1: establish the upstream contracts

- Inspect current official launcher's client manifests and download sources for each game and OS. Record versions, architectures, artifact formats, integrity metadata, update behavior, executable paths, and required launch parameters.
- Verify how each official client accepts a Jagex session and character; do not assume RuneLite's credential environment contract applies unchanged.
- Investigate Jagex's current Linux launcher beta before choosing Wine for OSRS. Verify actual game-client artifacts independently of launcher availability.
- Run isolated launch probes for official OSRS and RS3 on each OS. For Linux RS3, identify graphics and shared-library requirements; for Wine, establish the required version and prefix setup.
- Deliver a documented platform matrix with verified sources and launch contracts. Any unsupported combination has a concrete reason and is disabled in the UI.

## Phase 2: introduce shared client management

- Add explicit game and client identifiers: OSRS with RuneLite or Official; RS3 with Official.
- Replace RuneLite-specific Play/status wiring with a client manager and separate adapters for RuneLite, official OSRS, and RS3. Share installation mechanics where behavior actually matches.
- Track installation, progress, errors, version, and child processes per client. Prevent duplicate launch for a character already running; allow different characters to run concurrently when supported.
- Preserve existing account storage, session renewal, tray behavior, RuneLite installation paths, and user settings. Old settings default to OSRS/RuneLite.
- Reuse staged downloads and atomic activation. Require upstream integrity metadata where provided; document any source lacking authenticated integrity metadata before choosing its verification policy. Keep the previous working installation when an update fails.
- Separate launch preparation from spawning: finish installation first, then renew the session immediately before launching. Keep credentials out of arguments and logs wherever the upstream protocol permits.
- Exit criterion: RuneLite still passes regression checks and live launch through the shared path on both OSes.

## Phase 3: add selection and installation UI

- Add game selection and the available clients for that game. Persist the last choice per game, with character selection retained.
- Show selected client's version and states: not installed, preparing/downloading, ready, running, or failed with a retry action.
- First Play installs missing components automatically. Provide client-scoped repair and uninstall actions; repair preserves game settings and caches. Uninstall describes what it removes and never deletes saved accounts.
- Show unsupported OS/architecture combinations clearly. Offer actionable dependency errors instead of a generic launch failure.

## Phase 4: implement official OSRS

- Add Windows artifact installation, update checks, session handoff, logs, and actual game-process tracking.
- Implement the Linux route established in Phase 1. If Wine is required, use a Cabbage-owned prefix and a deliberate runtime source/version policy; preserve the prefix across client updates.
- Validate fresh install, subsequent launch, update, interrupted download, repair, expired session, client exit, and launcher exit on both OSes.

## Phase 5: implement RS3

- Add separate Windows and Linux adapters using verified upstream artifacts and session handoff.
- Resolve Linux dependencies with an explicit supported runtime strategy. Never silently replace system libraries or require a system package installation as part of Play.
- Apply the same installation, update, recovery, process tracking, and live acceptance checks as official OSRS.

## Phase 6: release acceptance and packaging

- Run all six client/OS combinations on real desktops. Linux automated tests do not substitute for Windows live tests.
- Validate multiple saved accounts, character switching, simultaneous different characters, session expiry, network loss, installation paths with spaces, restart after an interrupted update, and tray preferences.
- Check logs for credential exposure and test that remote login pages cannot invoke local launcher commands.
- Package Cabbage independently for Windows and Linux, with its own identity and persistent build/preview locations. Keep installed HapticScape untouched.
- Record known platform limits and verified client/runtime versions. Ship only combinations that pass installation and actual authenticated gameplay acceptance.

## Scope boundaries

Existing installation import, HDOS, Steam linking, custom executables, and adopting clients after a Cabbage restart are follow-up features. Launcher self-updates remain separate from game-client updates. Authentication compatibility work continues if the Cloudflare loop recurs; client expansion should not rewrite the working authentication flow without a specific need.

## Current primary reference

Jagex's [launcher and client download links](https://support.runescape.com/hc/en-gb/articles/44160372424849-Jagex-Launcher-and-Game-Client-download-links), checked 2026-10-12, list a Linux launcher beta. This confirms launcher availability, not the OSRS client's Linux implementation or its authentication contract. Phase 1 must verify those details.

## Research progress

See [official-client-contracts.md](official-client-contracts.md) for the inspected Jagex Linux beta, verified OSRS content reconstruction, and RS3 bootstrap/dependency findings. Static research is complete for these artifacts; isolated runtime probes and authenticated Windows/Linux acceptance remain outstanding. The shared manager and OSRS adapter are implemented; RS3 contracts are documented in official-rs3.md.

## Shared manager milestone

The shared registry/manager and bottom-row client selection are implemented. Official OSRS is implemented and the user confirmed Linux launching. Official RS3 is implemented for testing. See client-manager.md. Existing RuneLite paths and account/browser state are preserved. Regression tests pass on Windows and Linux; the user confirmed Linux launches for all three clients.

## RS3 adapter milestone

The Linux adapter downloads binaryType=4 from the current Jagex configuration, checks the decompressed client against the published CRC, and installs private pinned OpenSSL 1.1 libraries. It implements the launcher connection and tracks the game through an independent Cabbage host process. Windows verifies the official bootstrap through Authenticode and tracks its descendants through a job object. Neither route is authenticated gameplay acceptance yet. See official-rs3.md.

The user confirmed RS3 launching and the dedicated GPU preference on Linux. Client repair/uninstall controls are implemented as right-click actions on the bottom client row and documented in client-maintenance.md. Remaining work includes Windows live acceptance for all adapters and signed launcher update installation. Automatic Wine installation remains deferred by user preference.

## Recovery and launcher-update audit

Recovery cases and fixes are recorded in recovery-testing.md. Launcher release checks use only birdturtle/Cabbage. The separate game host, Linux AppImage and Windows EXE packaging, and portable/per-user installation are implemented. Signed update installation and installed-upgrade recovery remain in launcher-updates.md.
