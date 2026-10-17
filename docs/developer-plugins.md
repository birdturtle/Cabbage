# Local RuneLite developer plugins

This opt-in feature is for private testing of local, unreviewed RuneLite plugins. It is not Plugin Hub approval or a sandbox. A selected plugin executes inside RuneLite and can access the same client/account session. Only use JARs from a developer you trust.

## For testers

1. Download the plugin-only JAR supplied by the developer.
2. Open **Settings → Advanced → Developer plugins · RuneLite**.
3. Select **Add plugin JAR…**, choose the file, and enable **Launch RuneLite with developer plugins**.
4. Select RuneLite and your character on Home, then Play normally. The first launch downloads the official client runtime.
5. Enable the plugin in RuneLite if its author leaves it disabled by default.

No IDE, Gradle, system Java setup, terminal command, or credential-file copying is needed. Cabbage reuses its managed Java and existing Jagex sign-in/session renewal. It passes the character session through environment variables, just as normal RuneLite launches do; it never creates a credentials.properties file.

Each entry has an enabled checkbox and Remove action. Changes take effect on the next launch; they do not unload code from a running client. Remove deletes the selection, not the downloaded source JAR. Missing/changed JARs stop developer launches with an actionable error. To install an updated file, remove the old entry and add the new JAR explicitly.

Turn the global toggle off to launch ordinary RuneLite. On first use, Cabbage copies the existing `.runelite/profiles2` configuration profiles (including their active profile and Hub plugin list), legacy settings, `plugin-data`, and `resource-packs-repository` into a stable beta home. RuneLite downloads the selected Hub plugins itself. It does not copy credentials, account session files, unrelated sideloaded JARs, caches or logs, and does not follow symlinks. The copy is local, bounded to 20,000 entries / 512 MiB, and never overwrites the original setup.

The beta home stays the same across plugin JAR changes, preserving settings you make during testing. The normal and beta copies do not synchronize afterward. This avoids cross-platform symlink permissions and beta changes leaking into the normal profile. First-time copying and changes to the staged plugin set require managed RuneLite processes to be closed. Subsequent launches with the same JARs do not replace the staging folder. Closing Cabbage leaves an already-running game alone.

## For plugin authors

Supply a **plugin-only** JAR with non-RuneLite dependencies included. Do not bundle RuneLite, its launcher, JUnit, Mockito, a full development client, or test classes. Cabbage validates the archive without executing it. The limits are 16 selected plugins, 32 MiB per JAR, 64 MiB combined enabled JARs, 128 MiB expanded archive contents and 20,000 archive entries.

An optional manifest attribute `Cabbage-RuneLite-Version: 1.13.2` declares the exact client version used to compile the plugin. If present, Cabbage refuses a different runtime and asks for an updated plugin. Without it, compatibility remains the author's responsibility; valid archive structure cannot prove API compatibility. Runtime load/initialization errors appear in **Settings → Advanced → Logs → RuneLite launch**. Cabbage cannot guarantee a plugin starts merely because Java started successfully.

WikiPane supplies `./gradlew verifyDeveloperPluginJar`: the `build/libs/*-developer-plugin.jar` includes WikiPane and jsoup, excludes RuneLite/test code, and records its resolved compile-time RuneLite version.

## Implementation

- Normal RuneLite still uses Cabbage's unchanged official launcher path.
- The developer path reads `https://static.runelite.net/bootstrap.json`, downloads only matching-platform artifacts from `https://repo.runelite.net/`, and verifies each manifest SHA256 and byte count before activation. HTTPS certificate validation is required; redirects are not followed. The bootstrap also requires its official RSA/SHA256 detached signature, checked against the signing certificate distributed by RuneLite’s launcher. Cached manifests retain and reverify that signature.
- Runtime artifacts are content-addressed. A verified cached manifest/artifacts can be reused if the manifest endpoint is unavailable. Missing artifacts still require connectivity. Previous runtime versions/profiles are retained; automatic garbage collection is not implemented.
- The stable private beta profile contains copies of exactly the selected, hash-verified plugin files in `.runelite/sideloaded-plugins`. No archive is extracted into the user's filesystem.
- Cabbage starts `net.runelite.client.RuneLite` directly with assertions and `--developer-mode`, through the managed Java process. The regular launcher intentionally suppresses developer mode, so adding a flag to its ordinary command would not work.
- There is no injected agent, replacement RuneLite class, core patch, custom plugin classloader, or general browser/game interception. RuneLite's own `PluginManager.loadSideLoadPlugins` performs discovery/loading.
- Native file picking is confined to the local Cabbage window through a checked backend command. Remote authentication pages receive no new permissions.

### Verification

Run the existing UI suite with `npm run check` and Rust tests with `cargo test --locked --manifest-path src-tauri/Cargo.toml`.

An explicit network/package test downloads verified artifacts and stages a real plugin, without starting a game:

```sh
CABBAGE_TEST_PLUGIN_JAR=/absolute/path/to/plugin.jar \
CABBAGE_TEST_JAVA=/absolute/path/to/full-jdk/bin/java \
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  developer_plugins::tests::real_plugin_can_be_prepared_with_official_runtime -- --ignored --nocapture
```

The optional full-JDK argument also runs an offline class-discovery probe and RuneLite `--help`. It does not log in, start gameplay, or read a credential file. Automated preparation/class loading is not in-game acceptance.

Manual acceptance remains: Windows/Linux native picker, selected-character login, WikiPane functionality without Gradle, multiple plugins, disabled entries, missing/changed JARs, incompatible versions, normal-mode switching, and launch failure/log visibility. Never automate game input for these checks.

### Upstream references

- https://github.com/runelite/runelite/blob/master/runelite-client/src/main/java/net/runelite/client/plugins/PluginManager.java
- https://github.com/runelite/runelite/blob/master/runelite-client/src/main/java/net/runelite/client/RuneLite.java
- https://github.com/runelite/launcher/blob/master/src/main/java/net/runelite/launcher/Launcher.java
- https://static.runelite.net/bootstrap.json

## Failure handling

| Situation | Launcher behavior |
| --- | --- |
| Selected JAR missing or unreadable | Identifies the file and stops before client downloads or session renewal. |
| Selected JAR changed since it was added | Requires explicitly removing/re-adding the update. Staged files are not silently replaced. |
| No enabled JARs | Asks you to enable a JAR or turn developer mode off. |
| Declared RuneLite version mismatch | Identifies the plugin and both versions, and asks for an updated JAR before staging. |
| Developer mode off | Ignores saved JAR files, including missing ones, and uses the ordinary official launcher/profile. |
| Individual JAR disabled | Ignores that file during validation and staging. |
| RuneLite still running while changing the plugin set | Keeps the current JARs/profile unchanged and asks you to close RuneLite. The same plugin set can launch without restaging. |
| Invalid profile/staging destination | Reports the problem and leaves existing files unchanged. Failed first-copy staging is cleaned up. |
| Settings save or launch preparation fails | Shows the error, keeps the saved configuration accurate, and allows a retry. There is no automatic fallback that would silently run without requested developer plugins. |

Automated failure tests use temporary files and a harmless stand-in process, not the user's live RuneLite installation or account. Declared-version checks cannot prove a plugin's runtime compatibility when its author supplies no version metadata; plugin initialization errors remain visible in RuneLite launch logs.
