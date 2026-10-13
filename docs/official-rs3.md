# Official RuneScape integration

Implemented on feature/multi-client-launcher for Windows and Linux x64. Authenticated gameplay still needs user testing.

## Linux

Configuration: https://rs.config.runescape.com/k=5/l=0/jav_config.ws?binaryType=4. Jagex redirects this endpoint; redirects are limited to expected HTTPS download hosts. The configuration must select one rs2client binary with launcher protocol version 224. The current server version is 950. Downloads use the configuration's world server, file name and CRC. LZMA output and memory are bounded. The decompressed executable must match the published CRC and ELF x64 format. Installed files are checked against locally recorded SHA256 hashes.

Integrity policy: configuration and executable delivery rely on authenticated Jagex HTTPS. CRC is a corruption check, not a cryptographic signature. The legacy download_hash field is not verified by this adapter; it does not claim signed Linux game metadata. Windows has a separate Authenticode policy.

The actual game executable requires OpenSSL 1.1, SDL2, EGL and OpenGL. It does not require the old bootstrap's GTK2 dependency. Cabbage downloads the pinned Ubuntu libssl1.1 package from security.ubuntu.com and extracts only libssl.so.1.1, libcrypto.so.1.1 and the copyright notice. Package and library SHA256 pins are required. LD_LIBRARY_PATH applies only to the game process. No system packages or libraries are changed. SDL2 and graphics libraries must already be installed; missing dependencies are reported in the launch log. The current native client uses X11 through SDL, including XWayland on Wayland desktops.

Runtime package: libssl1.1_1.1.1f-1ubuntu2.24_amd64.deb. This is a legacy ABI required by the upstream client. Updating the compatibility package requires updating Cabbage's pins and rerunning acceptance.

A separate --rs3-host invocation of Cabbage creates private launcher FIFOs, sends protocol version 4 initialization with Cabbage-owned data/cache paths and window dimensions, acknowledges close requests, and waits for the actual game process. Credentials remain in the environment. Public configuration parameters are separate command arguments; no account tokens are added to arguments or handoff files. A startup acknowledgement prevents the UI reporting a host setup failure as a successful launch. The host bypasses GUI initialization and the single-instance plugin, so the IPC lifetime does not depend on Cabbage's window remaining open.

## Windows

Bootstrap: https://files.publishing.production.jxp.jagex.com/v2/games/RuneScape.exe. Download size is bounded. Windows PowerShell checks Authenticode validity and Jagex publisher identity before installation; the signed file's version is displayed. Inherited account credentials are removed from the verification process. The official bootstrap handles game updates.

The independent host belongs to a Windows job object before starting the bootstrap. Descendant game processes inherit that job. The host waits until only itself remains, so an exited bootstrap does not clear running status while the game remains open. No kill-on-close policy is set. Closing Cabbage leaves the host and game running.

## Validation

The real Linux adapter download and cached reuse passed for server version 950. Regression tests cover hostile configuration hosts, duplicate parameters, decompression/CRC rejection, preservation of an existing installation when activation fails, local corruption, protocol framing, and UI selection. An isolated startup probe loaded the native game without account credentials. Real graphics, login, client exit, launcher exit and concurrent characters still need desktop acceptance. Windows CI and Windows gameplay are separate checks.

## References

- Jagex configuration and Windows bootstrap URLs above.
- Ubuntu package: https://security.ubuntu.com/ubuntu/pool/main/o/openssl/libssl1.1_1.1.1f-1ubuntu2.24_amd64.deb
- Public-domain launcher protocol reference: https://github.com/syldrathecat/nxtlauncher
- MIT protocol reference: https://github.com/yoloium/rs3-launcher

Cabbage's Rust implementation was written separately. These references helped establish protocol fields; no proprietary Jagex launcher source or third-party implementation was copied into the repository.
