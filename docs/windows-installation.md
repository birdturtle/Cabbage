# Windows installation

The Windows download is one Cabbage EXE. Its first launch offers Install Cabbage or Keep portable. The UI needs the Microsoft Edge WebView2 runtime, which is present on Windows 11 and most Windows 10 installations. Runtime bootstrapping on systems without WebView2 is not implemented yet.

Install Cabbage copies the launcher to `%LOCALAPPDATA%/cabbage/launcher/Cabbage.exe`, creates the Start Menu shortcut with the cabbage icon, and registers Cabbage in the current user's Installed Apps list. No administrator rights are requested. Accounts and game files retain their existing locations. Open Cabbage from the Start Menu after installation.

Keep portable leaves the downloaded EXE in place without Start Menu or Installed Apps registration. It means no launcher installation; account storage still uses Windows Credential Manager and the existing user-data directory. Install remains available in Settings.

Uninstall from Settings or Windows Installed Apps removes the installed launcher, its owned shortcut, icon and registration. Saved accounts, preferences and games are retained. When the installed EXE is running, a separate PowerShell worker waits for Cabbage to exit before deleting it. It never terminates games. If another process still locks the file, cleanup retries briefly and leaves registration intact if it cannot remove the binary.

Build: `npm ci`, then `npm run build -- --no-bundle` on Windows. The Windows package workflow tests the backend and game helper and uploads one EXE as Cabbage-Windows-x64. Frontend resources, icon and game helper are embedded. Raw EXEs are not NSIS updater artifacts; signed replacement and recovery still need implementation before release.
