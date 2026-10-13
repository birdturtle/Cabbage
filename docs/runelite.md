# RuneLite integration

Cabbage fetches the stable official launcher JAR from the runelite/launcher GitHub repository. GitHub's SHA256 asset digest and expected download size are required. Java 21 JRE packages come from Adoptium's API and official temurin21-binaries repository, with the provider's SHA256 checksum.

Downloads and runtime extraction are staged before activation. Installed launchers and runtimes live in immutable hash directories under Cabbage's platform data directory. The current installation record is replaced only after verification. Failed first downloads can be retried. A failed release check can use an already verified local installation.

Play checks the selected account and renews its Jagex session, downloads missing components, and starts Java with the official launcher. Credentials are supplied as JX_SESSION_ID, JX_CHARACTER_ID and JX_DISPLAY_NAME environment variables. Inherited legacy access/refresh-token variables are removed. No credentials are passed as command-line arguments.

The launcher runs in REFLECT mode so the process Cabbage tracks is the client process. RuneLite still manages its game-client downloads and updates. Native launcher self-updates are disabled because Cabbage manages the launcher JAR. RuneLite uses its usual .runelite settings directory. Closing Cabbage leaves RuneLite running; process adoption after a Cabbage restart is not implemented.

The initial managed Java runtime is reused for launcher updates. Java-runtime update policy and garbage collection of previous component versions remain follow-up work.

Logs are saved to clients/runelite/launch.log under Cabbage's data directory. Process exits clear running status. Downloads report progress and failures on the main screen. Minimize after Play hides Cabbage to the tray when available, with ordinary minimization as fallback.

Supported download targets: Windows and Linux on x86_64 and aarch64. Windows acceptance still requires live sign-in and gameplay testing. The optional official-download integration test verifies real packages without signing in or starting a game.

## Upstream references

- https://github.com/runelite/launcher
- https://github.com/runelite/launcher/blob/master/src/main/java/net/runelite/launcher/Launcher.java
- https://github.com/runelite/launcher/blob/master/src/main/java/net/runelite/launcher/ReflectionLauncher.java
- https://api.adoptium.net/
