# Client repair and uninstall

Home displays OSRS, RuneLite and RS3 across its bottom row. Clicking an entry selects that client. Right-click or Shift+F10 opens Repair and Uninstall for that entry, independently of the selected client. Repair downloads and verifies fresh managed binaries before replacing the existing installation. Uninstall asks for confirmation and removes Cabbage-managed binaries. Saved accounts, browser profiles, launcher preferences, game settings and caches remain.

Managed paths per adapter:

| Client | Replaced or removed | Preserved |
| --- | --- | --- |
| RuneLite | launchers, runtimes, current.json | RuneLite settings and game caches outside the managed client directory |
| Official OSRS | versions, wine-version.txt, current.json | Wine prefix, registry, settings and caches |
| RuneScape | versions, runtime, current.json | native data directory; Windows game files/settings managed by Jagex's bootstrap |

The Windows RS3 action repairs or removes Cabbage's bootstrap. It does not delete the game installation managed externally by that bootstrap. No shared Jagex or RuneLite directories are deleted.

Repair is staged in the selected client's .repair directory. Download/verification failure leaves the active installation untouched. Activation first writes a recovery record, moves old artifacts into .maintenance/files, then moves verified replacements into place with current.json last. A separate commit marker is created after activation. Interrupted activation restores old artifacts before the next Play or maintenance action. Committed backups are cleanup only. Recovery records accept only the adapter's exact managed-path list.

The manager reserves a client during maintenance and refuses maintenance while any tracked character uses it. An independent process check catches managed games left running after Cabbage exits: Linux examines process executable paths and arguments; Windows queries process paths/command lines without printing them. Maintenance never kills a game. Running status can still be unadopted after restart, but maintenance checks do not rely only on the GUI manager.

The new command is restricted to the local main window by capability and backend URL checks. Client IDs and operation IDs are typed enums; callers cannot provide file paths. Uninstall remains available when dependencies are unavailable, allowing removal of broken installations.

Tests cover preserved game/account data, fresh activation, interrupted recovery, invalid recovery paths, safe link removal, reservations during launch and maintenance, running games after restart, confirmation/cancel/captured client UI behavior, and an isolated live RS3 repair/uninstall/reinstall cycle. Windows maintenance CI and user-facing acceptance remain separate checks.
