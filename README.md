# Cabbage

An unofficial launcher for Old School RuneScape and RuneScape on Windows and Linux.

Sign in with your Jagex account, choose a character and client, and press **Play**. Cabbage downloads the client for you and remembers your accounts for next time.

[Download Cabbage](https://github.com/birdturtle/Cabbage/releases/latest)

## Clients

| Client | Windows | Linux |
| --- | --- | --- |
| RuneLite | Native | Native |
| Official Old School RuneScape | Native | Requires Wine |
| RuneScape | Native | Native |

## Install

Download the file for your system from the [latest release](https://github.com/birdturtle/Cabbage/releases/latest). Both downloads are for x64 systems.

### Windows

1. Open `Cabbage-Windows-x64-1.0.0.exe`.
2. Choose **Install Cabbage** to add it to your Start Menu, or **Keep portable** to run it from the downloaded file.

Installation does not require administrator access. [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section) is required if it is not already installed.

### Linux

Make the AppImage executable and open it:

```sh
chmod +x Cabbage-Linux-x64-1.0.0.AppImage
./Cabbage-Linux-x64-1.0.0.AppImage
```

Choose **Install Cabbage** to add it to your application menu, or **Keep portable**. Installation does not require sudo.

Saved accounts require a desktop keyring with Secret Service support. Install Wine if you want to use the official Old School client. RuneLite includes its Java runtime.

## Play

1. Select **Add account** and sign in with your Jagex account.
2. Choose your account and character.
3. Select **OSRS**, **RuneLite** or **RS3** from the client cards at the bottom.
4. Press **Play**. The first launch downloads and installs the selected client.

You can add multiple accounts and switch between them. Use **Remove** to remove the selected account from Cabbage.

Right-click a client card to **Repair** or **Uninstall** that client.

## Settings

- Keep Cabbage open, hide it to the system tray, or close it after Play.
- Prefer your dedicated GPU; useful for those with integrated graphics and discrete graphics.
- Check for launcher updates on startup.
- Include beta updates if you want to try prereleases.

Closing the window hides Cabbage to the tray. Use the tray menu to restore the window or exit. Exiting Cabbage leaves running games open.

## Updates

Open **Updates**, select **Check for updates**, then **Install update and restart** when a new version is available.

Updates work with installed and portable copies. Your accounts, settings and downloaded clients are kept, and running games stay open.

## Data locations

Cabbage stores saved data in these folders, including when you choose **Keep portable**:

| Files | Windows | Linux |
| --- | --- | --- |
| Accounts and settings | `%APPDATA%\com.birdturtle.cabbage` | `~/.config/com.birdturtle.cabbage` |
| Downloaded clients and app data | `%APPDATA%\com.birdturtle.cabbage` | `~/.local/share/com.birdturtle.cabbage` |
| Installed launcher | `%LOCALAPPDATA%\cabbage\launcher` | `~/.local/share/cabbage/launcher` |

Linux paths follow `XDG_CONFIG_HOME` and `XDG_DATA_HOME` if you have changed those locations. Portable copies keep the launcher file where you put it.

## Uninstall

Use the installation section in **Settings** to uninstall Cabbage. On Windows, you can also use **Installed Apps**.

Uninstalling the launcher keeps saved accounts, settings and downloaded games. Uninstall individual clients from their right-click menus before removing the launcher if you want those removed too.

## Project

[Report a bug](https://github.com/birdturtle/Cabbage/issues) · [Build from source](docs/development.md)

BSD 2-Clause. See [LICENSE](LICENSE) and [third-party notices](THIRD-PARTY-NOTICES.md). Based on HapticScape's launcher.

Unofficial software. Not affiliated with Jagex or RuneLite.
