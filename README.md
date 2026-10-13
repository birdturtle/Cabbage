# Cabbage

An unofficial Linux launcher for RuneScape and Old School RuneScape.

The launcher source and history were extracted from HapticScape. Cabbage has its own application identity, settings, encrypted account vault, cache, and desktop entry.

## Development

This is the initial source extraction. HapticScape and LumBridge launch controls are still present and will be removed in the next phase. RuneLite and official-client integration follow after that.

```sh
npm ci
npm run check
cargo test --manifest-path src-tauri/Cargo.toml
npm run dev
```

Tauri development requires Node.js, Rust, WebKitGTK 4.1, GTK 3, and Secret Service on Linux.

Cabbage uses `com.birdturtle.cabbage` for application storage and `com.birdturtle.cabbage.jagex.v1` for its account-vault key. HapticScape accounts and settings are not imported.

## Planned clients

- Old School RuneScape: RuneLite and the official client.
- RuneScape: the official client.

## License

BSD 2-Clause. See LICENSE and THIRD-PARTY-NOTICES.md.

Unofficial software. Not affiliated with Jagex or RuneLite.
