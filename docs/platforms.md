# Platform support

Windows and Linux are equal targets for Cabbage. Login, account management, character selection, preferences, UI, and client management are shared.

Account files use AES-GCM. Windows Credential Manager and Linux Secret Service hold the encryption key through the existing platform-specific keyring backends. Cabbage does not directly call DPAPI.

RuneLite is the first client integration for both platforms. The inspected Jagex Linux beta supports OSRS through RuneLite. Official OSRS will use the Windows client directly on Windows and a managed Wine setup on Linux. RS3 uses a native game client with private OpenSSL compatibility libraries on Linux and a verified official bootstrap on Windows. Both RS3 routes need live acceptance. See official-client-contracts.md.

Launcher installation and updates will be separate from each client's installation and updates. Development and installed launchers must have distinct launch workflows so testing never redirects an existing installation.

Windows runtime acceptance remains necessary; passing Linux tests does not establish Windows behavior.

## Dedicated GPU preference

Linux Settings offers Prefer dedicated GPU, disabled by default. It applies to newly launched RuneLite, Official OSRS and RS3 processes. Systems with multiple DRM GPU devices use NVIDIA PRIME offload when the NVIDIA kernel module is present, or Mesa DRI_PRIME otherwise. Single-GPU systems retain normal selection. The RS3 host inherits the selected graphics environment and passes it to the game. Cabbage's UI and authentication browser are unaffected. Windows keeps OS-managed GPU selection and hides this Linux-specific control.

Verified on the development laptop: the offload environment reports NVIDIA GeForce RTX 4050 Laptop GPU in glxinfo. Confirm the restarted game's renderer separately. References: https://docs.nvidia.com/datacenter/tesla/driver-installation-guide/optimus-laptops-and-multi-gpu-desktop-systems.html and https://docs.mesa3d.org/envvars.html.

## After Play

Shared Windows/Linux preferences provide Keep launcher open, Hide to system tray, and Close launcher. Older minimizeOnPlay=true preferences migrate to Hide to system tray; false or absent preferences migrate to Keep launcher open. An explicit afterPlay choice takes precedence. The tray action retains ordinary minimization as fallback when a tray is unavailable. These actions execute only after preparation, session renewal, and process startup succeed. Close exits Cabbage without stopping game processes or the independent RS3 host.
