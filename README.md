# Cabbage

An unofficial Windows and Linux launcher for RuneScape and Old School RuneScape.

The launcher source and history were extracted from HapticScape. Cabbage has its own application identity, settings, encrypted account vault, cache, and desktop entry.

## Development

The source extraction and removal of inherited application controls are complete. Jagex sign-in, saved accounts, character selection, preferences, and tray behavior remain. RuneLite launch support is next. Play stays disabled until that integration is ready.

```sh
npm ci
npm run check
cargo test --manifest-path src-tauri/Cargo.toml
npm run dev
```

Tauri development requires Node.js and Rust. Linux builds need WebKitGTK 4.1, GTK 3, and Secret Service. Windows builds use the Microsoft C++ build tools and WebView2.

Saved accounts use AES-GCM encryption with a key held in Windows Credential Manager or Linux Secret Service.

Cabbage uses `com.birdturtle.cabbage` for application storage and `com.birdturtle.cabbage.jagex.v1` for its account-vault key. HapticScape accounts and settings are not imported.

## Planned clients

- Old School RuneScape: RuneLite and the official client.
- RuneScape: the official client.

## License

BSD 2-Clause. See LICENSE and THIRD-PARTY-NOTICES.md.

Unofficial software. Not affiliated with Jagex or RuneLite.
