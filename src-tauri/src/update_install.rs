use crate::{downloads, installation, updates};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const MAX_PACKAGE: usize = 512 * 1024 * 1024;
const PUBLIC_KEY: &[u8] = include_bytes!("../updater-public.pem");
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub platforms: BTreeMap<String, Artifact>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub busy: bool,
    pub percent: Option<u8>,
    pub message: String,
}
pub type SharedProgress = Arc<Mutex<Progress>>;
fn platform() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}
fn filename(version: &str) -> String {
    format!(
        "Cabbage-{}-x64-{version}.{}",
        if cfg!(windows) { "Windows" } else { "Linux" },
        if cfg!(windows) { "exe" } else { "AppImage" }
    )
}
fn release_url(tag: &str, name: &str) -> String {
    format!("https://github.com/birdturtle/Cabbage/releases/download/{tag}/{name}")
}
fn validate_manifest(token: &str, tag: &str) -> Result<Artifact, String> {
    verify_manifest(token, tag, PUBLIC_KEY)
}
fn verify_manifest(token: &str, tag: &str, public_key: &[u8]) -> Result<Artifact, String> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    let key = jsonwebtoken::DecodingKey::from_rsa_pem(public_key)
        .map_err(|_| "Invalid Cabbage update key.")?;
    let manifest = jsonwebtoken::decode::<Manifest>(token.trim(), &key, &validation)
        .map_err(|_| "Update signature is invalid.")?
        .claims;
    let v = updates::version(tag)?;
    if manifest.version != v.to_string() || tag != format!("v{v}") {
        return Err("Update version does not match its release.".into());
    }
    let artifact = manifest
        .platforms
        .get(platform())
        .ok_or("This release has no update for your platform.")?
        .clone();
    if artifact.url != release_url(tag, &filename(&manifest.version))
        || artifact.size == 0
        || artifact.size > MAX_PACKAGE
        || artifact.sha256.len() != 64
        || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid Cabbage update artifact.".into());
    }
    Ok(artifact)
}
pub fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Cabbage updater")
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5
                || url.scheme() != "https"
                || url.port().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
                || !matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.error("Invalid update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| "Cannot initialize update downloads.".into())
}
pub async fn artifact(http: &reqwest::Client, tag: &str) -> Result<Artifact, String> {
    updates::version(tag)?;
    let response = http
        .get(release_url(tag, "cabbage-updater.jwt"))
        .send()
        .await
        .map_err(|_| "Cannot download update information. Retry.")?
        .error_for_status()
        .map_err(|_| "This release does not contain an installable update.")?;
    let bytes = downloads::read_limited(response, 65536).await?;
    validate_manifest(
        std::str::from_utf8(&bytes).map_err(|_| "Invalid update signature.")?,
        tag,
    )
}
pub fn target() -> Result<PathBuf, String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err("Updates are not available for this architecture.".into());
    }
    installation::source().ok_or("Download the release to update this development build.".into())
}
pub async fn stage(
    http: &reqwest::Client,
    tag: &str,
    target: &Path,
    progress: &SharedProgress,
) -> Result<(PathBuf, String), String> {
    let artifact = artifact(http, tag).await?;
    stage_artifact(http, artifact, target, progress).await
}
async fn stage_artifact(
    http: &reqwest::Client,
    artifact: Artifact,
    target: &Path,
    progress: &SharedProgress,
) -> Result<(PathBuf, String), String> {
    let staged = target.with_extension(format!("update-{:x}", rand::random::<u64>()));
    let result = async {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options
            .open(&staged)
            .map_err(|_| "Cannot save update beside the launcher. Check folder permissions.")?;
        let mut response = http
            .get(&artifact.url)
            .send()
            .await
            .map_err(|_| "Cannot download update. Retry.")?
            .error_for_status()
            .map_err(|_| "Update download failed. Retry.")?;
        let mut received = 0usize;
        let mut hash = Sha256::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Update download interrupted. Retry.")?
        {
            received = received
                .checked_add(chunk.len())
                .ok_or("Update exceeds its size limit.")?;
            if received > artifact.size {
                return Err("Update exceeds its signed size.".into());
            }
            file.write_all(&chunk)
                .map_err(|_| "Cannot save update. Check free disk space.")?;
            hash.update(&chunk);
            *progress.lock().unwrap() = Progress {
                busy: true,
                percent: Some(((received as u64 * 100) / artifact.size as u64) as u8),
                message: "Downloading update…".into(),
            };
        }
        if received != artifact.size || format!("{:x}", hash.finalize()) != artifact.sha256 {
            return Err("Update failed verification. Retry the download.".into());
        }
        file.sync_all().map_err(|_| "Cannot save update.")?;
        Ok((staged.clone(), artifact.sha256.clone()))
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}
pub fn apply(
    target: PathBuf,
    staged: PathBuf,
    sha256: String,
    version: String,
    helper_root: &Path,
) -> Result<(), String> {
    let helper = crate::game_host::install(helper_root)?;
    let token = format!(
        "{:032x}{:032x}",
        rand::random::<u128>(),
        rand::random::<u128>()
    );
    let plan = cabbage_game_host::update::Plan {
        target,
        sha256,
        staged: staged.clone(),
        parent: std::process::id(),
        version,
        token,
    };
    let path = cabbage_game_host::update::companion(&staged, "plan");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| "Cannot prepare update restart.")?;
    file.write_all(&serde_json::to_vec(&plan).map_err(|_| "Cannot prepare update restart.")?)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot save update restart.")?;
    drop(file);
    let mut command = std::process::Command::new(helper);
    command.args([std::ffi::OsStr::new("--update-launcher"), path.as_os_str()]);
    crate::clients::configure_credentials(&mut command, &[]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| "Cannot start update helper.")?;
    let ready = cabbage_game_host::update::companion(&staged, "ready");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if fs::read(&ready).is_ok_and(|bytes| bytes == plan.token.as_bytes()) {
            return Ok(());
        }
        if child
            .try_wait()
            .map_err(|_| "Cannot track update helper.")?
            .is_some()
            || std::time::Instant::now() >= deadline
        {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(&ready);
            return Err("Update helper could not prepare the restart. Retry the update.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
pub fn acknowledge_startup() {
    let Some(ack) = std::env::var_os("CABBAGE_UPDATE_ACK").map(PathBuf::from) else {
        return;
    };
    let Some(target) = std::env::var_os("CABBAGE_UPDATE_TARGET").map(PathBuf::from) else {
        return;
    };
    let token = std::env::var("CABBAGE_UPDATE_TOKEN").unwrap_or_default();
    if std::env::var("CABBAGE_UPDATE_VERSION").ok().as_deref() != Some(env!("CARGO_PKG_VERSION"))
        || token.len() != 64
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
        || ack.parent() != target.parent()
        || !ack.is_absolute()
        || installation::source().and_then(|p| fs::canonicalize(p).ok())
            != fs::canonicalize(target).ok()
    {
        return;
    }
    let _ = fs::write(ack, token);
    for name in [
        "CABBAGE_UPDATE_ACK",
        "CABBAGE_UPDATE_TARGET",
        "CABBAGE_UPDATE_TOKEN",
        "CABBAGE_UPDATE_VERSION",
    ] {
        std::env::remove_var(name);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_or_corrupt_downloads_leave_existing_launcher_and_can_retry() {
        use std::io::{Read, Write};
        for (wire, success) in [
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort"
                    .as_slice(),
                false,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbad!".as_slice(),
                false,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngood".as_slice(),
                true,
            ),
        ] {
            let root = std::env::temp_dir().join(format!(
                "cabbage update download {:x}",
                rand::random::<u64>()
            ));
            fs::create_dir(&root).unwrap();
            let target = root.join("launcher");
            fs::write(&target, b"old").unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let wire = wire.to_vec();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                stream.read(&mut request).unwrap();
                stream.write_all(&wire).unwrap();
            });
            let artifact = Artifact {
                url: format!("http://{address}/update"),
                sha256: format!("{:x}", Sha256::digest(b"good")),
                size: 4,
            };
            let progress = SharedProgress::default();
            let result = tauri::async_runtime::block_on(stage_artifact(
                &reqwest::Client::new(),
                artifact,
                &target,
                &progress,
            ));
            assert_eq!(result.is_ok(), success);
            assert_eq!(fs::read(target).unwrap(), b"old");
            if success {
                assert_eq!(fs::read(result.unwrap().0).unwrap(), b"good");
                assert_eq!(progress.lock().unwrap().percent, Some(100));
            } else {
                assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            }
            server.join().unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn valid_signature_still_requires_exact_version_platform_url_size_and_hash() {
        let private = include_bytes!("test-fixtures/oauth-test-only.pem");
        let public = include_bytes!("test-fixtures/update-test-public.pem");
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(private).unwrap();
        let mut manifest = Manifest {
            version: "0.1.0-rc.2".into(),
            platforms: BTreeMap::from([(
                platform().into(),
                Artifact {
                    url: release_url("v0.1.0-rc.2", &filename("0.1.0-rc.2")),
                    sha256: "a".repeat(64),
                    size: 20,
                },
            )]),
        };
        let sign = |manifest: &Manifest| {
            jsonwebtoken::encode(
                &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
                manifest,
                &key,
            )
            .unwrap()
        };
        let token = sign(&manifest);
        assert!(verify_manifest(&token, "v0.1.0-rc.2", public).is_ok());
        assert!(validate_manifest(&token, "v0.1.0-rc.2").is_err());
        assert!(verify_manifest(&token, "v0.1.0-rc.3", public).is_err());
        manifest.platforms.get_mut(platform()).unwrap().url =
            "https://github.com/birdturtle/HapticScape/releases/download/v0.1.0-rc.2/other.exe"
                .into();
        assert!(verify_manifest(&sign(&manifest), "v0.1.0-rc.2", public).is_err());
        manifest.platforms.get_mut(platform()).unwrap().url =
            release_url("v0.1.0-rc.2", &filename("0.1.0-rc.2"));
        manifest.platforms.get_mut(platform()).unwrap().size = MAX_PACKAGE + 1;
        assert!(verify_manifest(&sign(&manifest), "v0.1.0-rc.2", public).is_err());
        manifest.platforms.clear();
        assert!(verify_manifest(&sign(&manifest), "v0.1.0-rc.2", public).is_err());
    }
    #[test]
    fn unsigned_wrong_algorithm_and_foreign_manifests_are_rejected() {
        assert!(validate_manifest("unsigned", "v0.1.0-rc.2").is_err());
        let manifest = Manifest {
            version: "0.1.0-rc.2".into(),
            platforms: BTreeMap::new(),
        };
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &manifest,
            &jsonwebtoken::EncodingKey::from_secret(b"wrong"),
        )
        .unwrap();
        assert!(validate_manifest(&token, "v0.1.0-rc.2").is_err());
    }
}
