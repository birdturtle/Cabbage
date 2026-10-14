# Building Cabbage

Tauri development requires Node.js and Rust. Linux builds need WebKitGTK 4.1, GTK 3 and Secret Service. Windows builds need the Microsoft C++ build tools and WebView2.

```sh
npm ci
npm run check
cargo test --locked --manifest-path dependency-helper/Cargo.toml
cargo test --locked --manifest-path game-host/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run dev
```

Build the Linux AppImage:

```sh
npm run build -- --bundles appimage
```

Build the Windows executable on Windows:

```sh
npm run build -- --no-bundle
```

See [launcher updates](launcher-updates.md) for release signing and update testing.
