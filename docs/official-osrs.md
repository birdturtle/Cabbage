# Official Old School client

The Official option downloads the Windows x64 client from Jagex on Windows and Linux. Linux requires Wine on PATH and uses a separate Cabbage Wine prefix. Cabbage does not install Wine yet. ARM is unsupported for this client.

Publishing documents use RS256 signatures checked against a pinned Jagex publishing key. A key change requires a Cabbage update. Downloads use the Jagex HTTPS CDN, verify every decompressed chunk, and retain the complete file listing, including companion DLLs. Installation records contain per-file hashes and are replaced after successful staging. Existing files are checked before reuse.

The shared manager downloads the client before refreshing the selected account session. Credentials are passed through the process environment. Browser login and protected account storage are unchanged. Windows starts osclient.exe directly; Linux starts it through Wine with its own prefix. Setup output and launch output stay in the client directory.

Validation: the live signed download and cached reuse passed for version 241.4. A Wine 11.19 startup probe stayed running without missing DLL errors. Authenticated gameplay, desktop graphics, and Windows launching still need acceptance testing. Official RS3 remains pending.
