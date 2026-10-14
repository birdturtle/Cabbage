# Windows dependency setup

Working branch: `feature/windows-dependency-setup`. Not part of the 1.0.0 release.

## Launcher startup

Before creating a WebView, Cabbage checks Microsoft's documented WebView2 runtime registry locations. Missing, empty, zero and malformed versions are rejected. If WebView2 is absent, a native Windows consent prompt offers installation. Declining exits before any download.

The Evergreen bootstrapper is downloaded from Microsoft's official link. HTTPS redirects are restricted to Microsoft domains. Downloads are bounded, staged in a unique folder and checked with Windows Authenticode. A valid signature from Microsoft Corporation is required. A read-only file handle denies modification/deletion while verifying and running the installer. WebView2 setup runs without requested elevation, allowing its per-user installation behavior. Readiness is checked again before starting the launcher UI.

## Official Old School client

On Windows x64, the client requires System32 copies of msvcp140.dll, vcruntime140.dll, vcruntime140_1.dll and vcomp140.dll. Availability is cached at startup rather than probing every UI poll. These checks establish file presence, not a guarantee that graphics or the game will work.

When missing, the selected client shows Install requirements and Check again near Play. Installation requires a confirmation dialog. Only the typed official OSRS client ID is accepted by the backend; neither download URLs nor executable paths are supplied by the frontend. Launching is checked again in Rust before any game preparation.

The official Microsoft x64 Visual C++ installer is downloaded and signature-checked using the same path as WebView2. Windows presents its UAC prompt; Cabbage never collects an administrator password. Setup uses passive progress and does not request an automatic reboot. Cancellation, failure and restart-required outcomes have distinct messages. The checks run again afterward. Users may also install the runtime manually and select Check again.

Setup progress and errors stay in the existing launch area. Launcher updates wait for dependency setup to finish. Elevated system installers are not killed on timeout or launcher exit; users are told to finish setup and recheck. Linux dependency installation is unchanged and will be handled by the later terminal helper.

## Verification

Cross-platform tests cover trusted download hosts, installer outcomes, consent before installation, cancellation, selected-client targeting and progress/retry UI. Windows-only tests reject an unsigned executable, validate registry version strings and simulate a cancelled UAC prompt without installing packages.

Windows and Linux CI passed. The Windows package build passed. The user tested the working-branch executable in a new WinBoat Windows VM and reported that setup works. Individual cases were not separately reported, so cancellation, UAC denial and network interruption still warrant targeted acceptance checks. No release tag was created for this work.
