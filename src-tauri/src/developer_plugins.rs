//! Opt-in local JARs using RuneLite's existing developer-mode sideloader.
//! Never patches RuneLite, changes the user's normal profile, or handles login tokens on disk.
use crate::{clients::Progress, runelite};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

const MAX_JAR: u64 = 32 * 1024 * 1024;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeveloperPlugins {
    pub enabled: bool,
    pub jars: Vec<LocalPlugin>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlugin {
    pub path: String,
    pub name: String,
    pub sha256: String,
    pub enabled: bool,
    #[serde(default)]
    pub runelite_version: Option<String>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn jar_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|_| {
        "Plugin file is missing or unreadable. Remove it and add the downloaded JAR again."
    })?;
    if !file
        .metadata()
        .map_err(|_| "Cannot inspect plugin file.")?
        .is_file()
    {
        return Err("Choose a regular JAR file.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_JAR + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read plugin JAR.")?;
    if bytes.len() as u64 > MAX_JAR {
        return Err("Plugin JAR exceeds the 32 MiB limit.".into());
    }
    Ok(bytes)
}
pub fn inspect(path: &Path) -> Result<LocalPlugin, String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
    {
        return Err("Choose a plugin .jar file.".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "Cannot find the selected plugin JAR.")?;
    let bytes = jar_bytes(&canonical)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .map_err(|_| "This file is not a valid JAR archive.")?;
    if archive.len() > 20_000 {
        return Err("Plugin JAR has too many entries.".into());
    }
    let mut has_class = false;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| "Cannot inspect plugin JAR entries.")?;
        let name = entry.name();
        if entry.enclosed_name().is_none() || name.contains('\\') {
            return Err("Plugin JAR contains an unsafe path.".into());
        }
        if name.starts_with("net/runelite/")
            || name.starts_with("org/junit/")
            || name.starts_with("org/mockito/")
        {
            return Err(
                "Choose a plugin-only JAR, not a bundled RuneLite client or test build.".into(),
            );
        }
        total = total
            .checked_add(entry.size())
            .ok_or("Plugin JAR is too large.")?;
        if total > 128 * 1024 * 1024 {
            return Err("Expanded plugin JAR is too large.".into());
        }
        has_class |= name.ends_with(".class");
    }
    if !has_class {
        return Err("This JAR contains no plugin classes.".into());
    }
    let runelite_version = if let Ok(mut entry) = archive.by_name("META-INF/MANIFEST.MF") {
        let mut text = String::new();
        entry
            .by_ref()
            .take(64 * 1024)
            .read_to_string(&mut text)
            .map_err(|_| "Invalid plugin manifest.")?;
        text.lines().find_map(|line| {
            line.strip_prefix("Cabbage-RuneLite-Version: ")
                .map(|v| v.trim().to_owned())
        })
    } else {
        None
    };
    Ok(LocalPlugin {
        path: canonical.to_string_lossy().into_owned(),
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        sha256: digest(&bytes),
        enabled: true,
        runelite_version,
    })
}
#[derive(Clone, Serialize, Deserialize)]
struct Platform {
    name: String,
    #[serde(default)]
    arch: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Artifact {
    name: String,
    path: String,
    hash: String,
    size: u64,
    #[serde(default)]
    platform: Vec<Platform>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Bootstrap {
    version: String,
    artifacts: Vec<Artifact>,
}
#[derive(Serialize, Deserialize)]
struct SignedBootstrap {
    json: String,
    signature: Vec<u8>,
}
fn signed_manifest(signed: &SignedBootstrap) -> Result<Bootstrap, String> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(include_bytes!("../runelite.crt"))
        .map_err(|_| "Invalid RuneLite signing certificate.")?;
    let certificate = pem
        .parse_x509()
        .map_err(|_| "Invalid RuneLite signing certificate.")?;
    let key = jsonwebtoken::DecodingKey::from_rsa_der(
        certificate.public_key().subject_public_key.data.as_ref(),
    );
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&signed.signature);
    if !jsonwebtoken::crypto::verify(
        &signature,
        signed.json.as_bytes(),
        &key,
        jsonwebtoken::Algorithm::RS256,
    )
    .map_err(|_| "RuneLite runtime manifest signature failed.")?
    {
        return Err("RuneLite runtime manifest signature failed.".into());
    }
    serde_json::from_str(&signed.json).map_err(|_| "Invalid RuneLite runtime manifest.".into())
}
fn matches_platform(artifact: &Artifact, os: &str, arch: &str) -> bool {
    let os = if os == "windows" { "win" } else { os };
    let arch = if arch == "x86_64" { "amd64" } else { arch };
    artifact.platform.is_empty()
        || artifact
            .platform
            .iter()
            .any(|p| p.name == os && p.arch.as_deref().is_none_or(|a| a == arch))
}
fn validate(bootstrap: &Bootstrap) -> Result<(), String> {
    semver::Version::parse(&bootstrap.version).map_err(|_| "Invalid RuneLite runtime version.")?;
    if bootstrap.artifacts.is_empty() || bootstrap.artifacts.len() > 128 {
        return Err("Invalid RuneLite runtime manifest.".into());
    }
    let mut names = std::collections::HashSet::new();
    let mut total = 0;
    for a in &bootstrap.artifacts {
        let url = url::Url::parse(&a.path).map_err(|_| "Invalid RuneLite artifact URL.")?;
        if url.scheme() != "https"
            || url.host_str() != Some("repo.runelite.net")
            || !url.username().is_empty()
            || url.password().is_some()
            || !a.name.ends_with(".jar")
            || a.name.contains(['/', '\\', ':'])
            || a.name.starts_with('.')
            || !names.insert(&a.name)
            || a.hash.len() != 64
            || !a.hash.bytes().all(|b| b.is_ascii_hexdigit())
            || a.size == 0
            || a.size > 64 * 1024 * 1024
        {
            return Err("Unsafe RuneLite runtime metadata.".into());
        }
        total += a.size;
    }
    if total > 256 * 1024 * 1024 {
        return Err("RuneLite runtime is too large.".into());
    }
    if !bootstrap
        .artifacts
        .iter()
        .any(|a| a.name == format!("client-{}.jar", bootstrap.version))
    {
        return Err("RuneLite runtime is missing its client.".into());
    }
    Ok(())
}
fn verified(path: &Path, a: &Artifact) -> bool {
    fs::metadata(path)
        .ok()
        .is_some_and(|m| m.is_file() && m.len() == a.size)
        && fs::read(path)
            .ok()
            .is_some_and(|bytes| digest(&bytes) == a.hash)
}
pub struct Prepared {
    pub home: PathBuf,
    pub classpath: std::ffi::OsString,
}
pub fn enabled_for(config: &DeveloperPlugins, client: crate::clients::ClientId) -> bool {
    config.enabled && client == crate::clients::ClientId::Runelite
}
/// A disabled developer mode must never touch saved JAR paths or block an ordinary launch.
pub fn preflight(config: &DeveloperPlugins) -> Result<(), String> {
    selected_jars(config).map(|_| ())
}
fn selected_jars(config: &DeveloperPlugins) -> Result<Vec<(&LocalPlugin, Vec<u8>)>, String> {
    if !config.enabled {
        return Ok(Vec::new());
    }
    let selected: Vec<_> = config.jars.iter().filter(|p| p.enabled).collect();
    if selected.is_empty() {
        return Err("Add and enable a plugin JAR, or turn developer plugins off.".into());
    }
    if selected.len() > 16 {
        return Err("At most 16 developer plugins may be enabled.".into());
    }
    // Validate all local files before making network requests or changing staging.
    let mut plugins = Vec::new();
    let mut total = 0usize;
    for plugin in selected {
        let bytes = jar_bytes(Path::new(&plugin.path))
            .map_err(|error| format!("{}: {error}", plugin.name))?;
        if digest(&bytes) != plugin.sha256 {
            return Err(format!(
                "{} changed. Remove it and add the updated JAR again.",
                plugin.name
            ));
        }
        total = total
            .checked_add(bytes.len())
            .ok_or("Selected plugins are too large.")?;
        if total > 64 * 1024 * 1024 {
            return Err("Selected plugin JARs exceed the combined 64 MiB limit.".into());
        }
        plugins.push((plugin, bytes));
    }
    Ok(plugins)
}
fn check_versions(plugins: &[(&LocalPlugin, Vec<u8>)], version: &str) -> Result<(), String> {
    for (plugin, _) in plugins {
        if plugin
            .runelite_version
            .as_ref()
            .is_some_and(|v| v != version)
        {
            return Err(format!("{} targets RuneLite {}, but the current runtime is {}. Ask for an updated plugin JAR.",
                plugin.name, plugin.runelite_version.as_ref().unwrap(), version));
        }
    }
    Ok(())
}
pub async fn prepare(
    root: &Path,
    config: &DeveloperPlugins,
    progress: &Mutex<Progress>,
    normal_home: &Path,
) -> Result<Prepared, String> {
    if !config.enabled {
        return Err("Developer plugins are disabled.".into());
    }
    let plugins = selected_jars(config)?;
    let base = root.join("developer");
    fs::create_dir_all(base.join("artifacts"))
        .map_err(|_| "Cannot create developer runtime directory.")?;
    progress.lock().unwrap().message = "Preparing RuneLite developer runtime…".into();
    let http = reqwest::Client::builder()
        .user_agent("Cabbage/1.1 developer-runtime")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot initialize runtime downloads.")?;
    let manifest_path = base.join("signed-bootstrap.json");
    let fresh = async {
        let response = http
            .get("https://static.runelite.net/bootstrap.json")
            .send()
            .await
            .map_err(|_| "Cannot reach RuneLite runtime manifest.")?
            .error_for_status()
            .map_err(|_| "RuneLite runtime manifest is unavailable.")?;
        let bytes = crate::downloads::read_limited(response, 512 * 1024)
            .await
            .map_err(|_| "Cannot read RuneLite runtime manifest.")?;
        let response = http
            .get("https://static.runelite.net/bootstrap.json.sha256")
            .send()
            .await
            .map_err(|_| "Cannot download RuneLite manifest signature.")?
            .error_for_status()
            .map_err(|_| "RuneLite manifest signature is unavailable.")?;
        let signature = crate::downloads::read_limited(response, 4096)
            .await
            .map_err(|_| "Cannot read RuneLite manifest signature.")?;
        let signed = SignedBootstrap {
            json: String::from_utf8(bytes).map_err(|_| "Invalid RuneLite manifest text.")?,
            signature,
        };
        signed_manifest(&signed).map_err(|_| "RuneLite manifest signature failed.")?;
        Ok::<_, &str>(signed)
    }
    .await;
    let signed = match fresh {
        Ok(signed) => signed,
        Err(error) => serde_json::from_slice::<SignedBootstrap>(
            &fs::read(&manifest_path).map_err(|_| error.to_owned())?,
        )
        .map_err(|_| error.to_owned())?,
    };
    let bootstrap = signed_manifest(&signed)?;
    validate(&bootstrap)?;
    check_versions(&plugins, &bootstrap.version)?;
    let mut classpath = Vec::new();
    for a in bootstrap
        .artifacts
        .iter()
        .filter(|a| matches_platform(a, std::env::consts::OS, std::env::consts::ARCH))
    {
        let path = base
            .join("artifacts")
            .join(format!("{}-{}", a.hash, a.name));
        if !verified(&path, a) {
            progress.lock().unwrap().message = format!("Downloading developer runtime: {}", a.name);
            let mut response = http
                .get(&a.path)
                .send()
                .await
                .map_err(|_| "Cannot download RuneLite developer runtime.")?
                .error_for_status()
                .map_err(|_| "RuneLite developer artifact is unavailable.")?;
            let staging = path.with_extension(format!("{:x}.part", rand::random::<u64>()));
            let result = async {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&staging)
                    .map_err(|_| "Cannot stage RuneLite artifact.")?;
                let mut bytes = 0;
                let mut hash = Sha256::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| "Runtime download interrupted.")?
                {
                    bytes += chunk.len() as u64;
                    if bytes > a.size {
                        return Err("Runtime download exceeds expected size.");
                    }
                    hash.update(&chunk);
                    file.write_all(&chunk)
                        .map_err(|_| "Cannot save RuneLite artifact.")?;
                }
                if bytes != a.size || format!("{:x}", hash.finalize()) != a.hash {
                    return Err("RuneLite artifact checksum failed.");
                }
                drop(file);
                if path.exists() {
                    fs::remove_file(&path)
                        .map_err(|_| "Cannot replace damaged RuneLite artifact.")?;
                }
                fs::rename(&staging, &path).map_err(|_| "Cannot activate RuneLite artifact.")?;
                Ok(())
            }
            .await;
            let _ = fs::remove_file(staging);
            result.map_err(str::to_owned)?;
        }
        classpath.push(path);
    }
    fs::write(&manifest_path, serde_json::to_vec(&signed).unwrap())
        .map_err(|_| "Cannot save runtime manifest.")?;
    let home = base.join("home");
    if !home.exists() {
        crate::maintenance::ensure_closed(root).map_err(|_| {
            "Close RuneLite before copying your existing profile into the developer environment."
        })?;
        crate::developer_profile::initialize(&home, normal_home)?;
    }
    let jars: Vec<_> = plugins.into_iter().map(|(_, bytes)| bytes).collect();
    crate::developer_profile::activate_plugins(root, &home, &jars)?;
    let classpath = std::env::join_paths(classpath).map_err(|_| "Runtime classpath is invalid.")?;
    Ok(Prepared { home, classpath })
}
fn command(
    root: &Path,
    install: &runelite::Installed,
    prepared: &Prepared,
    credentials: &[(String, String)],
) -> Command {
    let mut command = Command::new(runelite::java_path(root, install));
    command
        .arg("-ea")
        .arg("-Xmx768m")
        .arg("-Xss2m")
        .arg("-XX:+DisableAttachMechanism")
        .arg("--add-opens=java.base/java.net=ALL-UNNAMED")
        .arg("--add-opens=java.base/java.io=ALL-UNNAMED")
        .arg(format!("-Duser.home={}", prepared.home.display()))
        .arg("-cp")
        .arg(&prepared.classpath)
        .arg("net.runelite.client.RuneLite")
        .arg("--developer-mode");
    crate::clients::configure_credentials(&mut command, credentials);
    command
}
pub fn spawn(
    root: &Path,
    install: &runelite::Installed,
    prepared: &Prepared,
    credentials: &[(String, String)],
    gpu: bool,
) -> Result<Child, String> {
    let mut command = command(root, install, prepared, credentials);
    crate::graphics::configure(&mut command, gpu);
    let log = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(root.join("launch.log"))
        .map_err(|_| "Cannot open RuneLite launch log.")?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone().map_err(|_| "Cannot clone launch log.")?,
        ))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|_| "RuneLite developer runtime could not start. Check its launch log.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Bootstrap {
        serde_json::from_str(include_str!("test-fixtures/runelite-bootstrap.json")).unwrap()
    }
    fn temp() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("cabbage plugin test {:x}", rand::random::<u64>()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn jar(path: &Path, classes: &[&str]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for name in classes {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn official_manifest_filters_os_and_architecture_and_rejects_untrusted_artifacts() {
        let mut b = fixture();
        validate(&b).unwrap();
        let names = |os, arch| {
            b.artifacts
                .iter()
                .filter(|a| matches_platform(a, os, arch))
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        };
        let linux = names("linux", "x86_64");
        assert!(linux.contains(&"lwjgl-3.3.2-natives-linux.jar".into()));
        assert!(!linux
            .iter()
            .any(|n| n.contains("windows") || n.contains("macos") || n.contains("arm64")));
        let arm = names("windows", "aarch64");
        assert!(arm.contains(&"lwjgl-3.3.2-natives-windows-arm64.jar".into()));
        assert!(arm.contains(&"lwjgl-3.3.2-natives-windows.jar".into()));
        b.artifacts[0].path = "https://evil.invalid/client.jar".into();
        assert!(validate(&b).is_err());
        let mut b = fixture();
        b.artifacts[0].name = "../client.jar".into();
        assert!(validate(&b).is_err());
        let mut b = fixture();
        b.artifacts[0].hash = "bad".into();
        assert!(validate(&b).is_err());
    }
    #[test]
    fn official_signature_is_required_and_detects_manifest_or_signature_changes() {
        let mut signed = SignedBootstrap {
            json: include_str!("test-fixtures/runelite-bootstrap.json").into(),
            signature: include_bytes!("test-fixtures/runelite-bootstrap.sig").to_vec(),
        };
        signed_manifest(&signed).unwrap();
        signed.json.push(' ');
        assert!(signed_manifest(&signed).is_err());
        signed.json.pop();
        signed.signature[0] ^= 1;
        assert!(signed_manifest(&signed).is_err());
    }
    #[test]
    fn selected_jars_are_validated_without_executing_them() {
        let root = temp();
        let path = root.join("my plugin.jar");
        jar(&path, &["com/example/Plugin.class"]);
        let inspected = inspect(&path).unwrap();
        assert!(inspected.enabled);
        assert_eq!(inspected.sha256, digest(&fs::read(&path).unwrap()));
        jar(&path, &["net/runelite/client/RuneLite.class"]);
        assert!(inspect(&path).unwrap_err().contains("plugin-only"));
        jar(&path, &["../outside.class"]);
        assert!(inspect(&path).is_err());
        fs::write(&path, b"not a jar").unwrap();
        assert!(inspect(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupted_runtime_is_not_reused() {
        let root = temp();
        let path = root.join("client.jar");
        fs::write(&path, b"original").unwrap();
        let a = Artifact {
            name: "client.jar".into(),
            path: "https://repo.runelite.net/client.jar".into(),
            hash: digest(b"original"),
            size: 8,
            platform: vec![],
        };
        assert!(verified(&path, &a));
        fs::write(&path, b"modified").unwrap();
        assert!(!verified(&path, &a));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_settings_default_to_normal_launch_and_developer_choices_round_trip() {
        let old: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.developer_plugins.enabled);
        let config = DeveloperPlugins {
            enabled: true,
            jars: vec![LocalPlugin {
                path: "C:/a folder/plugin.jar".into(),
                name: "plugin.jar".into(),
                sha256: "a".repeat(64),
                enabled: false,
                runelite_version: Some("1.13.2".into()),
            }],
        };
        let saved: DeveloperPlugins =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert!(saved.enabled);
        assert!(!saved.jars[0].enabled);
        assert_eq!(saved.jars[0].path, config.jars[0].path);
    }
    #[test]
    fn missing_changed_and_disabled_jars_have_recoverable_preflight_paths() {
        let root = temp();
        let path = root.join("my plugin.jar");
        jar(&path, &["com/example/Plugin.class"]);
        let mut config = DeveloperPlugins {
            enabled: true,
            jars: vec![inspect(&path).unwrap()],
        };
        preflight(&config).unwrap();
        fs::remove_file(&path).unwrap();
        let error = preflight(&config).unwrap_err();
        assert!(error.contains("my plugin.jar") && error.contains("missing or unreadable"));
        let runtime_root = root.join("runtime");
        let error = tauri::async_runtime::block_on(prepare(
            &runtime_root,
            &config,
            &Mutex::new(Progress::default()),
            &root.join("normal"),
        ))
        .err()
        .unwrap();
        assert!(error.contains("missing or unreadable"));
        assert!(!runtime_root.exists());
        jar(
            &path,
            &["com/example/Plugin.class", "com/example/NewVersion.class"],
        );
        assert!(preflight(&config).unwrap_err().contains("changed"));
        config.jars[0] = inspect(&path).unwrap();
        preflight(&config).unwrap();
        config.jars.push(LocalPlugin {
            path: root.join("missing.jar").to_string_lossy().into(),
            name: "disabled.jar".into(),
            sha256: "0".repeat(64),
            enabled: false,
            runelite_version: None,
        });
        preflight(&config).unwrap();
        fs::remove_file(&path).unwrap();
        config.enabled = false;
        preflight(&config).unwrap();
        assert!(!enabled_for(&config, crate::clients::ClientId::Runelite));
        config.enabled = true;
        assert!(enabled_for(&config, crate::clients::ClientId::Runelite));
        assert!(!enabled_for(
            &config,
            crate::clients::ClientId::OfficialOsrs
        ));
        assert!(!enabled_for(&config, crate::clients::ClientId::OfficialRs3));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn version_checks_use_declared_manifest_version_and_identify_the_incompatible_plugin() {
        let root = temp();
        let path = root.join("versioned.jar");
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        archive
            .start_file(
                "com/example/Plugin.class",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"fixture").unwrap();
        archive
            .start_file(
                "META-INF/MANIFEST.MF",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(b"Manifest-Version: 1.0\r\nCabbage-RuneLite-Version: 1.13.2\r\n\r\n")
            .unwrap();
        archive.finish().unwrap();
        let config = DeveloperPlugins {
            enabled: true,
            jars: vec![inspect(&path).unwrap()],
        };
        let jars = selected_jars(&config).unwrap();
        check_versions(&jars, "1.13.2").unwrap();
        let error = check_versions(&jars, "1.13.3").unwrap_err();
        for text in ["versioned.jar", "1.13.2", "1.13.3", "updated plugin JAR"] {
            assert!(error.contains(text));
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn enabled_mode_requires_selected_plugins_and_obeys_count_and_file_size_limits() {
        let mut config = DeveloperPlugins {
            enabled: true,
            jars: vec![],
        };
        assert!(preflight(&config).unwrap_err().contains("Add and enable"));
        let root = temp();
        let path = root.join("oversized.jar");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_JAR + 1).unwrap();
        drop(file);
        config.jars.push(LocalPlugin {
            path: path.to_string_lossy().into(),
            name: "oversized.jar".into(),
            sha256: "0".repeat(64),
            enabled: true,
            runelite_version: None,
        });
        assert!(preflight(&config).unwrap_err().contains("32 MiB"));
        config.jars = vec![config.jars[0].clone(); 17];
        assert!(preflight(&config).unwrap_err().contains("16"));
        config.jars = vec![config.jars[0].clone()];
        config.jars[0].enabled = false;
        assert!(preflight(&config).unwrap_err().contains("Add and enable"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "downloads official RuneLite runtime, stages a supplied plugin; does not start a game"]
    fn real_plugin_can_be_prepared_with_official_runtime() {
        let path = std::env::var("CABBAGE_TEST_PLUGIN_JAR").expect("Set CABBAGE_TEST_PLUGIN_JAR");
        let root = temp();
        let plugin = inspect(Path::new(&path)).unwrap();
        let config = DeveloperPlugins {
            enabled: true,
            jars: vec![plugin],
        };
        let state = Mutex::new(Progress::default());
        let runtime = tauri::async_runtime::block_on(prepare(
            &root,
            &config,
            &state,
            &root.join("normal home"),
        ))
        .unwrap();
        assert_eq!(
            fs::read(
                runtime
                    .home
                    .join(".runelite/sideloaded-plugins/plugin-0.jar")
            )
            .unwrap(),
            fs::read(&path).unwrap()
        );
        assert!(std::env::split_paths(&runtime.classpath).count() > 20);
        if let Ok(java) = std::env::var("CABBAGE_TEST_JAVA") {
            let probe = root.join("DeveloperPluginProbe.java");
            fs::write(
                &probe,
                include_str!("../../scripts/DeveloperPluginProbe.java"),
            )
            .unwrap();
            let mut command = Command::new(&java);
            command
                .arg("--class-path")
                .arg(&runtime.classpath)
                .arg(probe)
                .arg(&path);
            crate::clients::configure_credentials(&mut command, &[]);
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            println!("{}", String::from_utf8_lossy(&output.stdout));
            let mut help = Command::new(java);
            help.arg("-cp")
                .arg(&runtime.classpath)
                .arg("net.runelite.client.RuneLite")
                .arg("--help");
            crate::clients::configure_credentials(&mut help, &[]);
            let output = help.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        println!("Prepared runtime at {}", root.display());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn developer_launch_uses_direct_client_and_environment_authentication() {
        let installed:runelite::Installed=serde_json::from_value(serde_json::json!({
            "version":"test", "jar_hash":"a".repeat(64), "runtime_hash":"b".repeat(64), "java":"bin/java"
        })).unwrap();
        let prepared = Prepared {
            home: PathBuf::from("a folder/developer home"),
            classpath: std::ffi::OsString::from("a classpath with spaces"),
        };
        let command = command(
            Path::new("root"),
            &installed,
            &prepared,
            &[
                ("JX_SESSION_ID".into(), "private-session".into()),
                ("JX_CHARACTER_ID".into(), "private-character".into()),
            ],
        );
        let args = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"--developer-mode".into()));
        assert!(args.contains(&"-ea".into()));
        assert!(args.contains(&"net.runelite.client.RuneLite".into()));
        assert!(!args.contains(&"-jar".into()));
        assert!(args.contains(&"a classpath with spaces".into()));
        assert!(!args.iter().any(|a| a.contains("private-")));
        let env = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(env["JX_SESSION_ID"].as_deref(), Some("private-session"));
        assert_eq!(env["JX_ACCESS_TOKEN"], None);
    }
}
