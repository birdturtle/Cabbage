# Platform support

Windows and Linux are equal targets for Cabbage. Login, account management, character selection, preferences, UI, and client management are shared.

Account files use AES-GCM. Windows Credential Manager and Linux Secret Service hold the encryption key through the existing platform-specific keyring backends. Cabbage does not directly call DPAPI.

RuneLite is the first client integration for both platforms. Official OSRS will use the Windows client directly on Windows and a managed Wine setup on Linux. RS3 needs separate native client integrations and runtime testing on each platform.

Launcher installation and updates will be separate from each client's installation and updates. Development and installed launchers must have distinct launch workflows so testing never redirects an existing installation.

Windows runtime acceptance remains necessary; passing Linux tests does not establish Windows behavior.
