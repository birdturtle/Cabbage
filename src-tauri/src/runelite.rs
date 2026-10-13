use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const MAX_EXPANDED: u64 = 600 * 1024 * 1024;
const REPOSITORY: &str = "runelite/launcher";

use crate::clients::Progress as State;
#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Runtime {
    binary: Binary,
}
#[derive(Deserialize)]
struct Binary {
    package: Package,
}
#[derive(Deserialize)]
struct Package {
    link: String,
    checksum: String,
    size: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    jar_hash: String,
    runtime_hash: String,
    java: String,
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}
fn safe_path(path: &Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if path.as_os_str().is_empty()
        || text.contains('\\')
        || text.contains(':')
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Unsafe Java archive path.".into());
    }
    Ok(())
}
fn jar_path(root: &Path, installed: &Installed) -> PathBuf {
    root.join("launchers")
        .join(&installed.jar_hash)
        .join("RuneLite.jar")
}
fn java_path(root: &Path, installed: &Installed) -> PathBuf {
    root.join("runtimes")
        .join(&installed.runtime_hash)
        .join(&installed.java)
}
pub fn installed(root: &Path) -> Option<Installed> {
    let install: Installed =
        serde_json::from_slice(&fs::read(root.join("current.json")).ok()?).ok()?;
    if !valid_hash(&install.jar_hash)
        || !valid_hash(&install.runtime_hash)
        || safe_path(Path::new(&install.java)).is_err()
    {
        return None;
    }
    let jar = fs::read(jar_path(root, &install)).ok()?;
    if !jar.starts_with(b"PK")
        || format!("{:x}", Sha256::digest(jar)) != install.jar_hash
        || !java_path(root, &install).is_file()
    {
        return None;
    }
    Some(install)
}
fn choose_asset(release: &Release) -> Result<Asset, String> {
    if release.draft || release.prerelease {
        return Err("RuneLite returned an unpublished or prerelease version.".into());
    }
    if release.tag_name.is_empty()
        || !release
            .tag_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    {
        return Err("Invalid RuneLite release version.".into());
    }
    let assets: Vec<_> = release
        .assets
        .iter()
        .filter(|a| a.name == "RuneLite.jar")
        .collect();
    if assets.len() != 1 {
        return Err("The RuneLite release does not contain a unique launcher JAR.".into());
    }
    let asset = assets[0];
    if asset.browser_download_url
        != format!(
            "https://github.com/{REPOSITORY}/releases/download/{}/RuneLite.jar",
            release.tag_name
        )
    {
        return Err("The RuneLite download is outside its official repository.".into());
    }
    let hash = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .ok_or("RuneLite did not provide a SHA256 checksum.")?;
    if !valid_hash(hash) || asset.size == 0 || asset.size > MAX_DOWNLOAD {
        return Err("Invalid RuneLite download metadata.".into());
    }
    Ok(asset.clone())
}
fn platform(os: &str, arch: &str) -> Result<(&'static str, &'static str), String> {
    let os = match os {
        "linux" => "linux",
        "windows" => "windows",
        _ => return Err("RuneLite setup supports Windows and Linux.".into()),
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        _ => return Err("This CPU architecture is not supported.".into()),
    };
    Ok((os, arch))
}
fn progress(state: &Mutex<State>, text: String) {
    state.lock().unwrap().message = text;
}
fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Cabbage/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let host = url.host_str().unwrap_or("");
            if attempt.previous().len() >= 5 {
                return attempt.error("Too many download redirects");
            }
            if url.scheme() == "https"
                && (host == "github.com" || host.ends_with(".githubusercontent.com"))
            {
                attempt.follow()
            } else {
                attempt.error("Unexpected download redirect")
            }
        }))
        .build()
        .map_err(|_| "Cannot initialize client downloads.".into())
}
async fn download(
    http: &reqwest::Client,
    url: &str,
    hash: &str,
    size: u64,
    destination: &Path,
    state: &Mutex<State>,
    label: &str,
) -> Result<(), String> {
    if !valid_hash(hash) || size == 0 || size > MAX_DOWNLOAD {
        return Err("Invalid download checksum or size.".into());
    }
    let mut response = http
        .get(url)
        .send()
        .await
        .map_err(|_| format!("Cannot download {label}. Check your connection and retry."))?
        .error_for_status()
        .map_err(|_| format!("The {label} download is unavailable."))?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|_| "Cannot create download staging.")?;
    let mut total = 0u64;
    let mut digest = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| format!("The {label} download was interrupted. Retry to continue."))?
    {
        total += chunk.len() as u64;
        if total > size || total > MAX_DOWNLOAD {
            return Err("Download exceeds its expected size.".into());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .map_err(|_| "Cannot save the client download.")?;
        progress(
            state,
            format!("Downloading {label}... {}%", total * 100 / size),
        );
    }
    file.sync_all()
        .map_err(|_| "Cannot finish the client download.")?;
    if total != size || format!("{:x}", digest.finalize()) != hash.to_lowercase() {
        return Err(format!(
            "The {label} checksum did not match. Retry the download."
        ));
    }
    Ok(())
}
fn extract(archive: &Path, destination: &Path, windows: bool) -> Result<String, String> {
    fs::create_dir(destination).map_err(|_| "Cannot create Java extraction staging.")?;
    let mut total = 0u64;
    let mut count = 0usize;
    if windows {
        let mut archive = zip::ZipArchive::new(
            fs::File::open(archive).map_err(|_| "Cannot read the Java archive.")?,
        )
        .map_err(|_| "Invalid Java ZIP archive.")?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|_| "Invalid Java ZIP entry.")?;
            let path = PathBuf::from(entry.name().trim_end_matches('/'));
            safe_path(&path)?;
            count += 1;
            total = total
                .checked_add(entry.size())
                .ok_or("Java archive is too large.")?;
            if total > MAX_EXPANDED
                || count > 20_000
                || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err("Unsafe or oversized Java archive.".into());
            }
            let target = destination.join(path);
            if entry.is_dir() {
                fs::create_dir_all(target).map_err(|_| "Cannot extract Java directories.")?;
            } else {
                fs::create_dir_all(target.parent().unwrap())
                    .map_err(|_| "Cannot extract Java directories.")?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map_err(|_| "Duplicate or inaccessible Java file.")?;
                std::io::copy(&mut entry, &mut output).map_err(|_| "Cannot extract Java files.")?;
            }
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
            fs::File::open(archive).map_err(|_| "Cannot read the Java archive.")?,
        ));
        for entry in archive.entries().map_err(|_| "Invalid Java TAR archive.")? {
            let mut entry = entry.map_err(|_| "Invalid Java TAR entry.")?;
            let path = entry
                .path()
                .map_err(|_| "Invalid Java archive path.")?
                .into_owned();
            safe_path(&path)?;
            count += 1;
            total = total
                .checked_add(entry.size())
                .ok_or("Java archive is too large.")?;
            if total > MAX_EXPANDED || count > 20_000 {
                return Err("Java archive is too large.".into());
            }
            let kind = entry.header().entry_type();
            if kind.is_symlink() {
                let link = entry
                    .link_name()
                    .map_err(|_| "Invalid Java archive link.")?
                    .ok_or("Missing Java link target.")?;
                let mut depth = path.components().count() - 1;
                for component in link.components() {
                    match component {
                        Component::Normal(_) => depth += 1,
                        Component::ParentDir if depth > 1 => depth -= 1,
                        Component::CurDir => (),
                        _ => return Err("Java archive link escapes its runtime directory.".into()),
                    }
                }
                if link.to_string_lossy().contains('\\') || link.to_string_lossy().contains(':') {
                    return Err("Invalid Java archive link.".into());
                }
            } else if !(kind.is_file() || kind.is_dir()) {
                return Err("Unsupported Java archive entry.".into());
            }
            for ancestor in path.ancestors().skip(1) {
                if fs::symlink_metadata(destination.join(ancestor))
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    return Err("Java archive entry passes through a link.".into());
                }
            }
            if fs::symlink_metadata(destination.join(&path))
                .is_ok_and(|m| !kind.is_dir() || m.file_type().is_symlink())
            {
                return Err("Duplicate Java archive entry.".into());
            }
            if !entry
                .unpack_in(destination)
                .map_err(|_| "Cannot extract Java files.")?
            {
                return Err("Unsafe Java archive path.".into());
            }
        }
    }
    let folders: Vec<_> = fs::read_dir(destination)
        .map_err(|_| "Cannot inspect Java installation.")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Cannot inspect Java installation.")?;
    if folders.len() != 1
        || !folders[0]
            .file_type()
            .map_err(|_| "Cannot inspect Java installation.")?
            .is_dir()
    {
        return Err("Invalid Java runtime layout.".into());
    }
    let executable = folders[0]
        .path()
        .join("bin")
        .join(if windows { "java.exe" } else { "java" });
    if !executable.is_file() {
        return Err("The Java runtime executable is missing.".into());
    }
    Ok(executable
        .strip_prefix(destination)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/"))
}
fn activate_manifest(root: &Path, install: &Installed) -> Result<(), String> {
    crate::downloads::activate_json(root, install)
}
pub async fn ensure(root: &Path, state: &Mutex<State>) -> Result<Installed, String> {
    let (os, arch) = platform(std::env::consts::OS, std::env::consts::ARCH)?;
    fs::create_dir_all(root).map_err(|_| "Cannot create the RuneLite data directory.")?;
    let current = installed(root);
    progress(state, "Checking RuneLite...".into());
    let http = download_client()?;
    let latest = http
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/latest"
        ))
        .timeout(Duration::from_secs(20))
        .send()
        .await;
    let release = match latest {
        Ok(response) if response.status().is_success() => response
            .json::<Release>()
            .await
            .map_err(|_| "RuneLite returned invalid release information."),
        _ => Err("Cannot check RuneLite updates. Check your connection and retry."),
    };
    let release = match release {
        Ok(release) => release,
        Err(error) => return current.ok_or(error.into()),
    };
    let asset = choose_asset(&release)?;
    let hash = asset
        .digest
        .as_deref()
        .unwrap()
        .strip_prefix("sha256:")
        .unwrap()
        .to_lowercase();
    if let Some(current) = &current {
        if current.jar_hash == hash {
            return Ok(current.clone());
        }
    }
    let staging = root.join(format!(".download-{:x}", rand::random::<u64>()));
    fs::create_dir(&staging).map_err(|_| "Cannot create client staging.")?;
    let result = async {
        download(&http, &asset.browser_download_url, &hash, asset.size, &staging.join("RuneLite.jar"), state, "RuneLite").await?;
        if !fs::read(staging.join("RuneLite.jar")).map_err(|_| "Cannot inspect RuneLite download.")?.starts_with(b"PK") { return Err("RuneLite download is not a JAR.".into()); }
        let (runtime_hash, java) = if let Some(current) = &current { (current.runtime_hash.clone(), current.java.clone()) } else {
            progress(state, "Checking Java runtime...".into());
            let runtime: Vec<Runtime> = http.get(format!("https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture={arch}&image_type=jre&os={os}&vendor=eclipse"))
                .timeout(Duration::from_secs(30)).send().await.map_err(|_| "Cannot contact the Java runtime provider.")?.error_for_status().map_err(|_| "The Java runtime is unavailable for this platform.")?
                .json().await.map_err(|_| "The Java runtime provider returned invalid metadata.")?;
            if runtime.len() != 1 { return Err("The Java runtime provider returned ambiguous packages.".into()); }
            let package = &runtime[0].binary.package;
            let url = url::Url::parse(&package.link).map_err(|_| "Invalid Java runtime download URL.")?;
            if url.scheme() != "https" || url.host_str() != Some("github.com") || !url.path().starts_with(&format!("/adoptium/temurin21-binaries/releases/download/")) { return Err("Java runtime download is outside its official repository.".into()); }
            let runtime_hash = package.checksum.to_lowercase();
            download(&http, &package.link, &runtime_hash, package.size, &staging.join("java.archive"), state, "Java").await?;
            progress(state, "Installing Java...".into());
            let java = extract(&staging.join("java.archive"), &staging.join("runtime"), os == "windows")?;
            let mut command = Command::new(staging.join("runtime").join(&java));
            command.arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
            if !command.status().map_err(|_| "The downloaded Java runtime could not start.")?.success() { return Err("The downloaded Java runtime failed verification.".into()); }
            fs::create_dir_all(root.join("runtimes")).map_err(|_| "Cannot create the Java runtime directory.")?;
            let target = root.join("runtimes").join(&runtime_hash);
            if !target.exists() { fs::rename(staging.join("runtime"), &target).map_err(|_| "Cannot activate the Java runtime.")?; }
            (runtime_hash, java)
        };
        fs::create_dir_all(root.join("launchers")).map_err(|_| "Cannot create the RuneLite launcher directory.")?;
        let target = root.join("launchers").join(&hash);
        if target.exists() {
            // A damaged inactive download is never trusted just because its folder exists.
            let bytes = fs::read(target.join("RuneLite.jar")).map_err(|_| "A previous RuneLite download is incomplete.")?;
            if format!("{:x}", Sha256::digest(bytes)) != hash { return Err("A previous RuneLite download is damaged.".into()); }
        } else {
            fs::create_dir(&target).map_err(|_| "Cannot create the RuneLite launcher directory.")?;
            if let Err(_) = fs::rename(staging.join("RuneLite.jar"), target.join("RuneLite.jar")) {
                let _ = fs::remove_dir(&target);
                return Err("Cannot activate RuneLite.".into());
            }
        }
        let install = Installed { version: release.tag_name, jar_hash: hash, runtime_hash, java };
        activate_manifest(root, &install)?;
        Ok(install)
    }.await;
    let _ = fs::remove_dir_all(staging);
    result
}
fn launch_command(root: &Path, install: &Installed, credentials: &[(String, String)]) -> Command {
    let mut command = Command::new(java_path(root, install));
    command
        .arg("-ea")
        .arg("-jar")
        .arg(jar_path(root, install))
        .arg("--launch-mode=REFLECT")
        .arg("--noupdate");
    crate::clients::configure_credentials(&mut command, credentials);
    command
}
pub fn spawn(
    root: &Path,
    install: &Installed,
    credentials: &[(String, String)],
    prefer_gpu: bool,
) -> Result<Child, String> {
    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(root.join("launch.log"))
        .map_err(|_| "Cannot open the RuneLite launch log.")?;
    let mut command = launch_command(root, install, credentials);
    crate::graphics::configure(&mut command, prefer_gpu);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone()
                .map_err(|_| "Cannot open the RuneLite launch log.")?,
        ))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|_| "RuneLite could not start. Retry the launch.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release() -> Release {
        Release {
            tag_name: "2.8.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "RuneLite.jar".into(),
                browser_download_url:
                    "https://github.com/runelite/launcher/releases/download/2.8.0/RuneLite.jar"
                        .into(),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                size: 10,
            }],
        }
    }

    #[test]
    #[ignore = "downloads official RuneLite and Temurin packages"]
    fn official_downloads_install_and_cached_launch_does_not_require_system_java() {
        let root = std::env::temp_dir().join(format!(
            "cabbage-official-download-{:x}",
            rand::random::<u64>()
        ));
        let state = Mutex::new(State::default());
        let install = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert!(installed(&root).is_some());
        let output = Command::new(java_path(&root, &install))
            .arg("-version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("21."));
        let second = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert_eq!(second.jar_hash, install.jar_hash);
        assert_eq!(second.runtime_hash, install.runtime_hash);
        assert!(!fs::read_dir(&root).unwrap().any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".download-")));
        println!(
            "Verified official RuneLite {} and Java at {}",
            install.version,
            root.display()
        );
        // Keep verified files for an optional manual launch; no account data is used.
    }
    #[test]
    fn credentials_are_only_passed_in_the_environment_and_legacy_tokens_are_removed() {
        let install = Installed {
            version: "2.8.0".into(),
            jar_hash: "a".repeat(64),
            runtime_hash: "b".repeat(64),
            java: "jdk/bin/java".into(),
        };
        let credentials = vec![
            ("JX_SESSION_ID".into(), "test-session".into()),
            ("JX_CHARACTER_ID".into(), "test-character".into()),
            ("JX_DISPLAY_NAME".into(), "Test Player".into()),
        ];
        let command = launch_command(Path::new("data"), &install, &credentials);
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--launch-mode=REFLECT".to_string()));
        assert!(!args
            .iter()
            .any(|a| a.contains("test-session") || a.contains("test-character")));
        let env: std::collections::HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_string(),
                    v.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();
        assert_eq!(
            env.get("JX_SESSION_ID").unwrap().as_deref(),
            Some("test-session")
        );
        assert_eq!(env.get("JX_ACCESS_TOKEN").unwrap(), &None);
        assert_eq!(env.get("JX_REFRESH_TOKEN").unwrap(), &None);
    }
    #[test]
    fn corrupt_download_is_rejected_before_installation() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad")
                .unwrap();
        });
        let file = std::env::temp_dir().join(format!(
            "cabbage-corrupt-download-{:x}",
            rand::random::<u64>()
        ));
        let result = tauri::async_runtime::block_on(download(
            &reqwest::Client::new(),
            &format!("http://{address}/file"),
            &"a".repeat(64),
            3,
            &file,
            &Mutex::new(State::default()),
            "test package",
        ));
        assert!(result.unwrap_err().contains("checksum"));
        server.join().unwrap();
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn interrupted_download_can_retry_in_fresh_staging_without_changing_active_record() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for body in ["go", "good"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n{body}"
                )
                .unwrap();
            }
        });
        let root = std::env::temp_dir().join(format!(
            "cabbage download recovery {:x}",
            rand::random::<u64>()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("current.json"), b"old-record").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"good"));
        let state = Mutex::new(State::default());
        let http = reqwest::Client::new();
        let first = tauri::async_runtime::block_on(download(
            &http,
            &format!("http://{address}/package"),
            &digest,
            4,
            &root.join("partial"),
            &state,
            "test",
        ));
        assert!(first.is_err());
        assert_eq!(fs::read(root.join("current.json")).unwrap(), b"old-record");
        tauri::async_runtime::block_on(download(
            &http,
            &format!("http://{address}/package"),
            &digest,
            4,
            &root.join("retry"),
            &state,
            "test",
        ))
        .unwrap();
        assert_eq!(fs::read(root.join("retry")).unwrap(), b"good");
        worker.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_java_archive_extracts_to_a_managed_executable() {
        let root = std::env::temp_dir().join(format!(
            "cabbage-windows-runtime-{:x}",
            rand::random::<u64>()
        ));
        fs::create_dir(&root).unwrap();
        let archive = root.join("java.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("jdk/bin/java.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"test executable").unwrap();
        zip.finish().unwrap();
        assert_eq!(
            extract(&archive, &root.join("extract"), true).unwrap(),
            "jdk/bin/java.exe"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn official_release_metadata_rejects_foreign_or_duplicate_downloads() {
        let mut r = release();
        assert!(choose_asset(&r).is_ok());
        r.assets[0].browser_download_url = "https://example.com/RuneLite.jar".into();
        assert!(choose_asset(&r).is_err());
        let mut r = release();
        r.assets.push(r.assets[0].clone());
        assert!(choose_asset(&r).is_err());
        let mut r = release();
        r.assets[0].digest = None;
        assert!(choose_asset(&r).is_err());
    }
    #[test]
    fn runtime_platform_selection_is_explicit() {
        assert_eq!(platform("windows", "x86_64").unwrap(), ("windows", "x64"));
        assert_eq!(platform("linux", "aarch64").unwrap(), ("linux", "aarch64"));
        assert!(platform("linux", "i686").is_err());
    }
    #[test]
    fn paths_reject_cross_platform_traversal() {
        for path in ["../escape", "/absolute", "C:/evil", "a\\..\\evil"] {
            assert!(safe_path(Path::new(path)).is_err());
        }
    }
    #[test]
    fn zip_extraction_rejects_link_entries_and_traversal() {
        let root =
            std::env::temp_dir().join(format!("cabbage-runtime-test-{:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let archive = root.join("runtime.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();
        assert!(extract(&archive, &root.join("extract"), true).is_err());
        assert!(!root.join("escape").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
