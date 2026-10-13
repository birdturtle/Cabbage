# Cabbage

An unofficial Windows and Linux launcher for RuneScape and Old School RuneScape.

## Features

- Jagex sign-in with saved accounts and character selection.
- RuneLite, official Old School RuneScape, and RuneScape clients.
- Automatic client downloads.
- Repair and uninstall from each client's right-click menu.
- Portable use or installation with application-menu shortcuts.
- System tray support.
- Keep the launcher open, hide it to the tray, or close it after Play.
- Dedicated GPU preference on Linux.
- Release checks with optional beta updates.

## Installation

Download the build for your system from [Releases](https://github.com/birdturtle/Cabbage/releases).

### Windows

Open `Cabbage-Windows-x64-0.1.0-rc.1.exe` and choose **Install Cabbage** or **Keep portable**. Installation adds Cabbage to the Start Menu and Installed Apps without administrator access.

[Microsoft Edge WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section) is required. Install it first if it is missing.

### Linux

Make the AppImage executable and open it:

```sh
chmod +x Cabbage-Linux-x64-0.1.0-rc.1.AppImage
./Cabbage-Linux-x64-0.1.0-rc.1.AppImage
```

Choose **Install Cabbage** to add it to your application menu, or **Keep portable**. Installation does not require sudo.

A desktop keyring providing Secret Service is required for saved accounts. The official Old School client requires Wine. RuneLite and RuneScape run natively.

### Uninstall

Use **Settings > Installation > Uninstall Cabbage**. Windows installations can also be removed through Installed Apps. Saved accounts, preferences, games and caches are kept.

## Updates

The Updates page checks Cabbage releases. Enable **Include beta updates** to see release candidates.

Launcher update installation is not included in this RC. Download new builds from Releases and choose Install Cabbage. Close an older launcher before installing its replacement.

## Development

```sh
npm ci
npm run check
cargo test --manifest-path game-host/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run dev
```

Build the Linux AppImage with `npm run build -- --bundles appimage`. Build the Windows EXE with `npm run build -- --no-bundle` on Windows.

Tauri development requires Node.js and Rust. Linux builds need WebKitGTK 4.1, GTK 3 and Secret Service. Windows builds need the Microsoft C++ build tools and WebView2.

## License

BSD 2-Clause. See LICENSE and THIRD-PARTY-NOTICES.md. Based on HapticScape's launcher.

Unofficial software. Not affiliated with Jagex or RuneLite.
