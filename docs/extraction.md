# Launcher extraction

Completed: launcher-only history import, source license retention, Cabbage app ID and executable name, separate platform directories and keyring service, Cabbage desktop entry, update repository, and provisional icon.

Desktop registration is opt-in through linux/install-desktop-entry.sh. This extraction does not install a desktop entry or launch a GUI on the development machine.

The app ID determines Tauri settings, data, cache, and single-instance identity. The vault encryption purpose and keyring service are both separate from HapticScape. Existing accounts are not migrated.

HapticScape deep-link startup and desktop protocol registration are disabled. The unused implementation remains for removal in phase 2.

Remaining: remove HapticScape/LumBridge process controls, legacy migrations, inherited suite packaging, and old update formats. Replace them with independent client and launcher management. This is not yet a usable standalone game launcher.
