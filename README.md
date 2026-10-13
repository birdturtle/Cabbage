# Cabbage

An unofficial Windows and Linux launcher for RuneScape and Old School RuneScape.

The launcher source and history were extracted from HapticScape. Cabbage has its own application identity, settings, encrypted account vault, cache, and desktop entry.

## Development

The source extraction and removal of inherited application controls are complete. Jagex sign-in, saved accounts, character selection, preferences, and tray behavior remain. Play downloads the official RuneLite launcher and a Temurin Java 21 runtime, verifies their SHA256 checksums, renews the selected Jagex session, and launches the selected character. RuneLite handles its game-client updates. Windows and Linux are supported by the shared launch path.

```sh
npm ci
npm run check
cargo test --manifest-path src-tauri/Cargo.toml
npm run dev
```

Tauri development requires Node.js and Rust. Linux builds need WebKitGTK 4.1, GTK 3, and Secret Service. Windows builds use the Microsoft C++ build tools and WebView2.

Saved accounts use AES-GCM encryption with a key held in Windows Credential Manager or Linux Secret Service.

Cabbage uses `com.birdturtle.cabbage` for application storage and `com.birdturtle.cabbage.jagex.v1` for its account-vault key. HapticScape accounts and settings are not imported.

## Game and client selection

Choose Old School RuneScape or RuneScape, then a client. RuneLite and Official OSRS are available. Official OSRS uses Wine on Linux. The RS3 adapter is ready for testing with a native Linux client and the signed Windows launcher. Cabbage remembers the selected game and Old School client.

Launches are tracked per account and character. Different characters can have separate game sessions; launching the same character twice is blocked. Windows live acceptance and RS3 gameplay testing remain outstanding.

## Launcher updates

The Updates page checks Cabbage releases. Installing launcher updates is still under development.

## Client files

Right-click OSRS, RuneLite or RS3 at the bottom of Home to repair or uninstall it. Repair replaces managed client files. Uninstall keeps saved accounts, game settings and caches. Close the client before using either action.

## Planned clients

- Old School RuneScape: RuneLite and the official client.
- RuneScape: the official client.

## License

BSD 2-Clause. See LICENSE and THIRD-PARTY-NOTICES.md.

Unofficial software. Not affiliated with Jagex or RuneLite.
