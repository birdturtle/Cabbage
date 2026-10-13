# Platform support

Windows and Linux are equal targets for Cabbage. Login, account management, character selection, preferences, UI, and client management are shared.

Account files use AES-GCM. Windows Credential Manager and Linux Secret Service hold the encryption key through the existing platform-specific keyring backends. Cabbage does not directly call DPAPI.

RuneLite is the first client integration for both platforms. The inspected Jagex Linux beta supports OSRS through RuneLite. Official OSRS will use the Windows client directly on Windows and a managed Wine setup on Linux. RS3 uses a native game client with private OpenSSL compatibility libraries on Linux and a verified official bootstrap on Windows. Both RS3 routes need live acceptance. See official-client-contracts.md.

Launcher installation and updates will be separate from each client's installation and updates. Development and installed launchers must have distinct launch workflows so testing never redirects an existing installation.

Windows runtime acceptance remains necessary; passing Linux tests does not establish Windows behavior.
