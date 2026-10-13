# Official client research

Checked 2026-10-12. Research branch: `feature/multi-client-launcher`.

## Evidence and acceptance

These findings come from public Jagex downloads and metadata, inspected in `/tmp/cabbage-client-research`. No client was installed into a user installation and no account credentials were used. Static inspection and successful download verification are not authenticated gameplay acceptance.

The official Linux launcher beta inspected was version 0.1.8. Its AppImage came from the Linux link in [Jagex's download article](https://support.runescape.com/hc/en-gb/articles/44160372424849-Jagex-Launcher-and-Game-Client-download-links). The downloaded artifact's local SHA256 was `f3a6527c823022f0c7ac1448ad90a5a5f5122816ac4b78cc0e3a3ff67391984b`; this is a research receipt, not an independently published checksum.

## Platform matrix

| Client | Windows | Linux | Current evidence |
| --- | --- | --- | --- |
| RuneLite | Managed official JAR and Temurin | Managed official JAR and Temurin | Existing Cabbage integration; Linux live acceptance confirmed by user |
| Official OSRS | Windows x86_64 executable | Windows executable through isolated Wine runtime | Official production content reconstructed and every chunk digest checked; no live acceptance yet |
| RS3 | Jagex Windows bootstrap | Jagex native amd64 Debian bootstrap | Both bootstrap artifacts inspected; actual game download and launch acceptance remain unresolved |

The Linux launcher beta's definitions install RuneLite on Linux. They do not provide an official OSRS client. Its RS3 definition supports Windows and macOS, and rejects Linux. This is evidence about version 0.1.8, not proof that Jagex can never supply native official OSRS.

Use Wine as the current OSRS Linux route. Keep unsupported architectures disabled until upstream artifacts and actual runtime tests establish support. Do not expose RS3 Linux as working merely because a Debian download exists.

## Official OSRS delivery

Public production metadata:

- `https://jagex.akamaized.net/direct6/osrs-win/osrs-win.json`
- Production version observed: `241.4`.
- Production content ID: `ee922d81223fe26f24880d307c32a7e2d2a592ec06bedded60273cf2d34c7aaa`.
- Catalog: `/direct6/osrs-win/catalog/<content-id>/catalog.json` on the same HTTPS origin.

The environment, catalog, and metafile documents are JWS-shaped metadata with RS256 signatures and an embedded certificate. The inspected metafile signature verified against its embedded certificate, whose subject is Jagex Limited. This check alone does not establish certificate trust. The adapter must validate trusted transport and its signing policy; it must not trust arbitrary keys supplied by arbitrary metadata.

The catalog publishes plain-HTTP `jagex-akamai.aws.snxd.com` locations. Verified requests succeeded using the corresponding HTTPS `jagex.akamaized.net` location. Implement an explicit host mapping, not unrestricted URL rewriting or disabled TLS verification.

The metafile declares SHA256 chunk digests, file order and lengths, padding, and a piece URL template. The observed template is `pieces/{SubString:0,2,{TargetDigest}}/{TargetDigest}.solidpiece`, with base64 digests converted to hexadecimal. Each observed piece has a six-byte framing prefix followed by gzip data. With `hashPadding=false`, the SHA256 matches the decompressed bytes. All ten pieces were downloaded, decompressed, checked, and concatenated; the resulting length exactly matched the sum of the declared file lengths.

Preserve the entire file list, not only the executable:

| File | Bytes | Verified reconstructed SHA256 |
| --- | ---: | --- |
| `discord_game_sdk.dll` | 3,891,512 | `527768710ddb0953fce5eb1700c2566b6451135d76f1d0610b63907cd5ba94c5` |
| `osclient.exe` | 15,978,792 | `a2636a0144b9fed75fefa5de7d1c56a24fe16dcf0222c7e82e9f8bf8f187843f` |

Bound file sizes and decompression, validate paths, reject ambiguous executable selection, and require known framing/hash algorithms. Do not assume future piece framing or padding modes match the observed version.

`osclient.exe` is a Windows x86_64 PE executable. Inspection finds the `JX_SESSION_ID`, `JX_CHARACTER_ID`, `JX_DISPLAY_NAME`, and legacy access/refresh-token names. This supports the existing session-environment handoff design, but successful authentication still requires a live probe. The executable also supports `--configURI`; determine whether an override is necessary through that probe.

Imports include `MSVCP140.dll` and `VCOMP140.DLL`. A bare Wine binary is not a complete managed compatibility runtime policy. Validate Visual C++ support, graphics and audio, and a Cabbage-owned Wine prefix before enabling the client. Local Wine is 11.19, but it has not been accepted as Cabbage's shipped runtime.

## RS3 Windows

The inspected official beta downloads a standalone Windows bootstrap from:

`https://files.publishing.production.jxp.jagex.com/v2/games/RuneScape.exe`

It copies the executable into its managed client directory and launches it with Jagex session/character/display-name environment variables. Its generic spawn path does not terminate the game when the launcher closes.

The inspected file is a Windows x86_64 PE executable. It contains an Authenticode certificate naming Jagex Limited, issued by DigiCert. Certificate presence and local SHA256 are not a full Authenticode validation result. Implement Windows verification and confirm the bootstrap's update and child-process behavior before treating it as a verified installation.

Local research SHA256: `4e269a44fbfdbd7bacac9e0e96dab7b339406cff298ca318c5661c3160a17564`.

This bootstrap fetches additional game binaries. Tracking only its initial PID may lose the actual game process; the adapter needs a tested process-lifetime contract.

## RS3 Linux

Public Jagex package index:

`https://content.runescape.com/downloads/ubuntu/dists/trusty/non-free/binary-amd64/Packages`

Observed package: `runescape-launcher`, version `2.2.12`, architecture `amd64`. Package path:

`pool/non-free/r/runescape-launcher/runescape-launcher_2.2.12_amd64.deb`

Published size: 3,602,512 bytes. Published SHA256: `4e774ba986c15fff3f4215ccd86cdedab25afc6bb0491b11402d3a8d5ff428a2`. The downloaded package matched. It contains a `data.tar.xz` member and `usr/share/games/runescape-launcher/runescape`, an x86_64 ELF bootstrap.

The bootstrap links OpenSSL 1.1, GTK 2, X11 and related libraries. This development machine lacks `libssl.so.1.1`, `libcrypto.so.1.1`, and GTK 2. The package also declares SDL2 and OpenGL/EGL requirements. Do not replace system libraries or assume a current distribution satisfies this legacy ABI. Decide on a supported private runtime/container strategy, or use an explicitly supported alternative route after testing.

Binary strings show `--configURI`, a `codebase` setting, binary counts/names, and additional client download requests. This artifact is a bootstrap, not proof that the full game is bundled or launches. The actual game's session handling, update integrity, SDL/Wayland requirements, and process lifetime remain to be probed.

Requests without a binary type returned HTTP 404. A follow-up probe succeeded with `https://rs.config.runescape.com/k=5/l=0/jav_config.ws?binaryType=2`, and the corresponding www.runescape.com URL also responded. That response selects Windows `rs2client.exe`, with launcher version 224, subversion 1, server version 950, and `download_crc_0` / `download_hash_0` fields. The codebase comes from the response and must not be hardcoded to the observed world server.

This establishes a working Windows configuration response, not the correct Linux binary type or successful gameplay. Verify the bootstrap's platform selection, hash/signature interpretation, native Linux response, and actual game executable handoff before implementation. Do not pass the Windows binary type to the Linux adapter.

## Implementation order

1. Add the shared game/client manager and adapt the working RuneLite path, preserving current installation records, session renewal and accounts.
2. Download/prepare before renewing credentials, then pass only the selected account's current session to the process.
3. Add OSRS Windows installation from complete, verified direct6 content; add Linux Wine setup only after its runtime policy and launch probe pass.
4. Resolve RS3 bootstrap/configuration and private Linux dependencies before enabling RS3 options.
5. Test authenticated gameplay on real Windows and Linux desktops. Static inspection is not a substitute for these checks.

Keep the working browser profile and fallback branch intact. The login fixes are committed at `3496ddd`; Linux login and RuneLite launch are confirmed. Windows live login remains untested.

## References

- [Jagex official launcher downloads](https://support.runescape.com/hc/en-gb/articles/44160372424849-Jagex-Launcher-and-Game-Client-download-links)
- [Official Linux launcher beta](https://rs-launcher-updates.runescape.com/production/linux/x64/latest/jagex-launcher-beta-linux-x86_64.AppImage)
- [OSRS production metadata](https://jagex.akamaized.net/direct6/osrs-win/osrs-win.json)
- [RS3 Linux package index](https://content.runescape.com/downloads/ubuntu/dists/trusty/non-free/binary-amd64/Packages)
- [RS3 Windows bootstrap](https://files.publishing.production.jxp.jagex.com/v2/games/RuneScape.exe)

RSClient's client modules were consulted to locate public delivery endpoints. No third-party client implementation or proprietary launcher source was copied into Cabbage. Findings above were checked against live Jagex artifacts where stated.
