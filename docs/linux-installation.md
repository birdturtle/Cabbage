# Linux installation

Build: `npm ci`, then `npm run build -- --bundles appimage`. The result is in the Cargo target directory under `release/bundle/appimage`. Local source builds need the Tauri Linux build dependencies. CI builds on Ubuntu 22.04 and uploads Cabbage-Linux-x64 as a workflow artifact.

Make the downloaded AppImage executable and open it. Choose Install Cabbage to copy it to `$XDG_DATA_HOME/cabbage/launcher/Cabbage.AppImage` (normally `~/.local/share/cabbage/launcher/Cabbage.AppImage`) and add its application-menu entry and icon. No root access is needed. After installation, open Cabbage from the application menu.

Keep portable leaves the downloaded file in place and suppresses the first-launch prompt. Installation remains available in Settings. Development executables do not show the AppImage installation controls.

Settings > Installation > Uninstall Cabbage removes the installed launcher and its owned menu entry/icon. Accounts, preferences, game downloads, Wine prefixes and caches are kept. The original downloaded AppImage is also kept.

Installation writes an explicit AppImage ownership record. Future updater work must distinguish the running installed AppImage from a downloaded portable copy; an absent record must not imply package-manager ownership. Signed in-app update installation is a later step and is not implemented by this installer.

Acceptance: isolated install/reinstall/uninstall tests preserve account data. The local AppImage builds successfully and its embedded launcher/desktop entry were verified by extraction. First-launch integration and application-menu startup still require hands-on testing with the packaged artifact. Local builds use the development system libraries; release artifacts should use the Ubuntu CI build.
