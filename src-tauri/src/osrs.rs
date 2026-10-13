use crate::{
    clients::{configure_credentials, Progress},
    downloads,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const CDN: &str = "https://jagex.akamaized.net/direct6/osrs-win";
const SIGNING_KEY: &str = "c36341819050b50db28ffa9ce0f5a5268b324ec9ec75d77053c47786a421099c";
const MAX_METADATA: usize = 1024 * 1024;
const MAX_GAME: usize = 128 * 1024 * 1024;
const MAX_PIECE: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
struct Environments {
    environments: EnvironmentsByName,
}
#[derive(Deserialize)]
struct EnvironmentsByName {
    production: Production,
}
#[derive(Deserialize)]
struct Production {
    id: String,
    version: String,
}
#[derive(Deserialize)]
struct Catalog {
    id: String,
    metafile: String,
    config: CatalogConfig,
}
#[derive(Deserialize)]
struct CatalogConfig {
    remote: Remote,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Remote {
    base_url: String,
    piece_format: String,
    flags: String,
}
#[derive(Deserialize)]
struct Metafile {
    id: String,
    version: String,
    algorithm: String,
    pieces: Pieces,
    files: Vec<FileEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pieces {
    algorithm: String,
    digests: Vec<String>,
    hash_padding: bool,
}
#[derive(Clone, Deserialize)]
struct FileEntry {
    name: String,
    size: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    id: String,
    files: Vec<SavedFile>,
}
#[derive(Clone, Serialize, Deserialize)]
struct SavedFile {
    name: String,
    size: u64,
    sha256: String,
}

fn hash_ok(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn safe_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains(':')
        || Path::new(value)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid official client file path.".into());
    }
    Ok(())
}
fn decode_metadata<T: DeserializeOwned>(token: &str) -> Result<T, String> {
    if token.len() > MAX_METADATA {
        return Err("Official client metadata is too large.".into());
    }
    let header =
        jsonwebtoken::decode_header(token).map_err(|_| "Invalid official client metadata.")?;
    if header.alg != Algorithm::RS256 {
        return Err("Unsupported official client signing algorithm.".into());
    }
    let certificate = header
        .x5c
        .as_ref()
        .and_then(|chain| chain.first())
        .ok_or("Official client signing certificate is missing.")?;
    let certificate = STANDARD
        .decode(certificate)
        .map_err(|_| "Invalid official client signing certificate.")?;
    let (remainder, certificate) = x509_parser::parse_x509_certificate(&certificate)
        .map_err(|_| "Invalid official client signing certificate.")?;
    if !remainder.is_empty() {
        return Err("Invalid official client signing certificate.".into());
    }
    let key = certificate.public_key().subject_public_key.data.as_ref();
    if format!("{:x}", Sha256::digest(key)) != SIGNING_KEY {
        return Err(
            "Jagex's client signing key changed. Update Cabbage before downloading this client."
                .into(),
        );
    }
    let mut validation = Validation::new(Algorithm::RS256);
    // These are signed publishing documents, not account OAuth tokens.
    // Authorization comes from the pinned publishing key, not certificate-chain claims in the response.
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    jsonwebtoken::decode(token, &DecodingKey::from_rsa_der(key), &validation)
        .map(|data| data.claims)
        .map_err(|_| "Official client metadata signature did not verify.".into())
}
async fn response(http: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = http.get(url).send().await.map_err(|_| {
        "Cannot reach the official client download server. Check your connection and retry."
    })?;
    if !response.status().is_success() {
        return Err("The official client download is unavailable. Retry later.".into());
    }
    downloads::read_limited(response, limit).await
}
async fn metadata<T: DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T, String> {
    let bytes = response(http, url, MAX_METADATA).await?;
    decode_metadata(
        std::str::from_utf8(&bytes).map_err(|_| "Invalid official client metadata text.")?,
    )
}
fn normalize_cdn(url: &str) -> Result<String, String> {
    let mut url = url::Url::parse(url).map_err(|_| "Invalid official client download location.")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.scheme(), "http" | "https")
    {
        return Err("Invalid official client download location.".into());
    }
    match url.host_str() {
        Some("jagex-akamai.aws.snxd.com") => url
            .set_host(Some("jagex.akamaized.net"))
            .map_err(|_| "Invalid official client download location.")?,
        Some("jagex.akamaized.net") => (),
        _ => return Err("Official client download is outside the expected Jagex server.".into()),
    }
    url.set_scheme("https")
        .map_err(|_| "Invalid official client download location.")?;
    Ok(url.to_string())
}
fn validate_files(meta: &Metafile) -> Result<usize, String> {
    if meta.algorithm != "sha256" || meta.pieces.algorithm != "sha256" || meta.pieces.hash_padding {
        return Err("Unsupported official client content format.".into());
    }
    if meta.files.is_empty()
        || meta.files.len() > 256
        || meta.pieces.digests.is_empty()
        || meta.pieces.digests.len() > 512
    {
        return Err("Invalid official client content listing.".into());
    }
    let mut names = std::collections::HashSet::new();
    let mut total = 0u64;
    for file in &meta.files {
        safe_path(&file.name)?;
        if !names.insert(file.name.to_ascii_lowercase()) {
            return Err("Duplicate official client file.".into());
        }
        total = total
            .checked_add(file.size)
            .ok_or("Official client content is too large.")?;
        if total > MAX_GAME as u64 {
            return Err("Official client content is too large.".into());
        }
    }
    if !meta
        .files
        .iter()
        .any(|f| f.name == "osclient.exe" && f.size > 0)
    {
        return Err("The official game executable is missing.".into());
    }
    Ok(total as usize)
}
fn decode_piece(compressed: &[u8], expected: &[u8]) -> Result<Vec<u8>, String> {
    if !compressed.starts_with(&[0x6e, 0x2b, 0x3b, 0x61, 0x00, 0x02]) {
        return Err("Unsupported official client chunk format.".into());
    }
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&compressed[6..])
        .take(MAX_PIECE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Official client chunk is damaged.")?;
    if bytes.len() > MAX_PIECE {
        return Err("Official client chunk is too large.".into());
    }
    if expected.len() != 32 || Sha256::digest(&bytes).as_slice() != expected {
        return Err("Official client checksum did not match. Retry the download.".into());
    }
    Ok(bytes)
}
fn write_content(folder: &Path, meta: &Metafile, content: &[u8]) -> Result<Vec<SavedFile>, String> {
    if content.len() != validate_files(meta)? {
        return Err("Official client download does not match its file listing.".into());
    }
    let mut offset = 0usize;
    let mut files = Vec::new();
    for file in &meta.files {
        let end = offset
            .checked_add(file.size as usize)
            .ok_or("Official client file is too large.")?;
        let bytes = content
            .get(offset..end)
            .ok_or("Official client download is truncated.")?;
        let path = folder.join(&file.name);
        fs::create_dir_all(path.parent().unwrap())
            .map_err(|_| "Cannot create official client directories.")?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "Cannot save the official client files.")?;
        output
            .write_all(bytes)
            .map_err(|_| "Cannot save the official client files.")?;
        files.push(SavedFile {
            name: file.name.clone(),
            size: file.size,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        });
        offset = end;
    }
    if !content_is_x64(
        &fs::read(folder.join("osclient.exe"))
            .map_err(|_| "Cannot inspect the official client executable.")?,
    ) {
        return Err("The official client is not a supported Windows x64 executable.".into());
    }
    Ok(files)
}
fn content_is_x64(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || !bytes.starts_with(b"MZ") {
        return false;
    }
    let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
    bytes.get(offset..offset.saturating_add(6)) == Some(&[b'P', b'E', 0, 0, 0x64, 0x86][..])
}
pub fn installed(root: &Path) -> Option<Installed> {
    let installed: Installed =
        serde_json::from_slice(&fs::read(root.join("current.json")).ok()?).ok()?;
    if !hash_ok(&installed.id) || installed.files.is_empty() || installed.files.len() > 256 {
        return None;
    }
    let version = root.join("versions").join(&installed.id);
    if fs::symlink_metadata(&version)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let mut names = std::collections::HashSet::new();
    let mut total = 0u64;
    for file in &installed.files {
        safe_path(&file.name).ok()?;
        if !names.insert(file.name.to_ascii_lowercase()) || !hash_ok(&file.sha256) {
            return None;
        }
        total = total.checked_add(file.size)?;
        if total > MAX_GAME as u64 {
            return None;
        }
        let path = version.join(&file.name);
        for ancestor in path.ancestors().take_while(|p| *p != version) {
            if fs::symlink_metadata(ancestor)
                .ok()?
                .file_type()
                .is_symlink()
            {
                return None;
            }
        }
        let bytes = fs::read(&path).ok()?;
        if bytes.len() as u64 != file.size || format!("{:x}", Sha256::digest(&bytes)) != file.sha256
        {
            return None;
        }
    }
    if !content_is_x64(&fs::read(version.join("osclient.exe")).ok()?) {
        return None;
    }
    Some(installed)
}
fn progress(state: &Mutex<Progress>, text: impl Into<String>) {
    state.lock().unwrap().message = text.into();
}
pub async fn ensure(root: &Path, state: &Mutex<Progress>) -> Result<Installed, String> {
    fs::create_dir_all(root).map_err(|_| "Cannot create the official client directory.")?;
    let current = installed(root);
    progress(state, "Checking the official Old School client...");
    let http = reqwest::Client::builder()
        .user_agent("Cabbage/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot initialize official client downloads.")?;
    let environments: Environments = match metadata(&http, &format!("{CDN}/osrs-win.json")).await {
        Ok(value) => value,
        Err(error) => {
            // Connectivity failures may use an existing verified install. Signature failures never fall back silently.
            if error.starts_with("Cannot reach") {
                return current.ok_or(error);
            }
            return Err(error);
        }
    };
    let production = environments.environments.production;
    if !hash_ok(&production.id) || production.version.is_empty() || production.version.len() > 80 {
        return Err("Invalid official client release identifier.".into());
    }
    if let Some(current) = current {
        if current.id == production.id {
            return Ok(current);
        }
    }
    let catalog: Catalog = metadata(
        &http,
        &format!("{CDN}/catalog/{}/catalog.json", production.id),
    )
    .await?;
    if catalog.id != production.id
        || normalize_cdn(&catalog.metafile)?
            != format!("{CDN}/metafile/{}/metafile.json", production.id)
        || normalize_cdn(&catalog.config.remote.base_url)? != format!("{CDN}/")
        || catalog.config.remote.piece_format
            != "pieces/{SubString:0,2,{TargetDigest}}/{TargetDigest}.solidpiece"
        || catalog.config.remote.flags != "gzip"
    {
        return Err("Unsupported official client catalog format.".into());
    }
    let meta: Metafile = metadata(&http, &normalize_cdn(&catalog.metafile)?).await?;
    if meta.id != production.id || meta.version != production.version {
        return Err("Official client metadata does not match its release.".into());
    }
    let expected_size = validate_files(&meta)?;
    let mut content = Vec::with_capacity(expected_size);
    for (index, digest) in meta.pieces.digests.iter().enumerate() {
        let digest = STANDARD
            .decode(digest)
            .map_err(|_| "Invalid official client chunk digest.")?;
        if digest.len() != 32 {
            return Err("Invalid official client chunk digest.".into());
        }
        let hex = digest
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let compressed = response(
            &http,
            &format!("{CDN}/pieces/{}/{hex}.solidpiece", &hex[..2]),
            8 * 1024 * 1024,
        )
        .await?;
        let piece = decode_piece(&compressed, &digest)?;
        if content.len().saturating_add(piece.len()) > expected_size {
            return Err("Official client download exceeds its file listing.".into());
        }
        content.extend_from_slice(&piece);
        progress(
            state,
            format!(
                "Downloading the official Old School client... {}%",
                (index + 1) * 100 / meta.pieces.digests.len()
            ),
        );
    }
    progress(state, "Installing the official Old School client...");
    let versions = root.join("versions");
    fs::create_dir_all(&versions).map_err(|_| "Cannot create official client versions.")?;
    let staging = versions.join(format!(".download-{:x}", rand::random::<u64>()));
    fs::create_dir(&staging).map_err(|_| "Cannot create official client staging.")?;
    let result = (|| {
        let files = write_content(&staging, &meta, &content)?;
        let installed = Installed {
            id: production.id,
            version: production.version,
            files,
        };
        let destination = versions.join(&installed.id);
        // Repair an inactive damaged copy by replacing its whole directory; keep the active record unchanged on failure.
        let backup = versions.join(format!(".previous-{:x}", rand::random::<u64>()));
        let had_destination = destination.exists();
        if had_destination {
            fs::rename(&destination, &backup)
                .map_err(|_| "Cannot repair the official client files.")?;
        }
        if fs::rename(&staging, &destination).is_err() {
            if had_destination {
                let _ = fs::rename(&backup, &destination);
            }
            return Err("Cannot activate the official client files.".into());
        }
        downloads::activate_json(root, &installed)?;
        if had_destination {
            let _ = fs::remove_dir_all(backup);
        }
        Ok(installed)
    })();
    let _ = fs::remove_dir_all(staging);
    result
}
pub fn wine_path() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|p| p.join("wine"))
            .find(|p| {
                if !p.is_file() {
                    return false;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    return fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
                }
                #[cfg(not(unix))]
                {
                    true
                }
            })
    })
}
fn command(wine: &Path, prefix: &Path) -> Command {
    let mut command = Command::new(wine);
    command.env("WINEPREFIX", prefix).env("WINEDEBUG", "-all");
    configure_credentials(&mut command, &[]);
    command
}
fn wait_for_setup(command: &mut Command) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|_| "Wine could not start. Check your Wine installation.")?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(exit) = child.try_wait().map_err(|_| "Cannot check Wine setup.")? {
            return if exit.success() {
                Ok(())
            } else {
                Err("Wine setup failed. Check the Wine setup log in Cabbage's data folder.".into())
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Wine setup timed out. Retry setup after closing any Wine setup dialog.".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub fn prepare_wine(root: &Path, state: &Mutex<Progress>) -> Result<(), String> {
    if cfg!(windows) {
        return Ok(());
    }
    let wine = wine_path().ok_or("Install Wine to use the official Old School client on Linux.")?;
    let version = Command::new(&wine)
        .arg("--version")
        .output()
        .map_err(|_| "Cannot check the Wine version.")?;
    if !version.status.success() {
        return Err("Wine is not working. Check your Wine installation.".into());
    }
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    let prefix = root.join("wine-prefix");
    if fs::symlink_metadata(&prefix).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Cabbage's Wine prefix must be a regular directory.".into());
    }
    let marker = root.join("wine-version.txt");
    let libraries = ["kernel32.dll", "msvcp140.dll", "vcomp140.dll"];
    let ready = || {
        libraries
            .iter()
            .all(|name| prefix.join("drive_c/windows/system32").join(name).is_file())
    };
    if fs::read_to_string(&marker).ok().as_deref() == Some(&version) && ready() {
        return Ok(());
    }
    progress(
        state,
        "Preparing Wine for the official Old School client...",
    );
    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(root.join("wine-setup.log"))
        .map_err(|_| "Cannot open the Wine setup log.")?;
    let mut setup = command(&wine, &prefix);
    setup
        .args(["wineboot.exe", "-u"])
        .env("WINEDLLOVERRIDES", "mscoree,mshtml=")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone()
                .map_err(|_| "Cannot open the Wine setup log.")?,
        ))
        .stderr(Stdio::from(log));
    wait_for_setup(&mut setup)?;
    if !ready() {
        return Err("Wine is missing the Visual C++ libraries required by this client. Update Wine and retry.".into());
    }
    fs::write(marker, version).map_err(|_| "Cannot save Wine setup state.".to_string())?;
    Ok(())
}
fn launch_command(
    root: &Path,
    installed: &Installed,
    credentials: &[(String, String)],
    wine: Option<&Path>,
) -> Result<Command, String> {
    let folder = root.join("versions").join(&installed.id);
    let executable = folder.join("osclient.exe");
    let mut launch = if let Some(wine) = wine {
        let mut c = command(wine, &root.join("wine-prefix"));
        c.arg(&executable);
        c
    } else {
        Command::new(&executable)
    };
    launch.current_dir(&folder);
    configure_credentials(&mut launch, credentials);
    Ok(launch)
}
pub fn spawn(
    root: &Path,
    installed: &Installed,
    credentials: &[(String, String)],
    prefer_gpu: bool,
) -> Result<Child, String> {
    let wine = if cfg!(target_os = "linux") {
        Some(wine_path().ok_or("Install Wine to use the official Old School client on Linux.")?)
    } else {
        None
    };
    let mut launch = launch_command(root, installed, credentials, wine.as_deref())?;
    crate::graphics::configure(&mut launch, prefer_gpu);
    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(root.join("launch.log"))
        .map_err(|_| "Cannot open the official client launch log.")?;
    launch
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            log.try_clone()
                .map_err(|_| "Cannot open the official client launch log.")?,
        ))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        launch.creation_flags(0x08000000);
    }
    launch
        .spawn()
        .map_err(|_| "The official Old School client could not start. Check its launch log.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_metadata_verifies_and_tampering_fails() {
        let token = include_str!("test-fixtures/osrs-environments.jws");
        let env: Environments = decode_metadata(token.trim()).unwrap();
        assert!(hash_ok(&env.environments.production.id));
        let mut parts: Vec<_> = token.trim().split('.').map(str::to_owned).collect();
        parts[1] = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"{}");
        assert!(decode_metadata::<Environments>(&parts.join(".")).is_err());
    }
    #[test]
    fn chunk_verification_rejects_bad_hash_or_format() {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(b"content").unwrap();
        let mut bytes = vec![0x6e, 0x2b, 0x3b, 0x61, 0x00, 0x02];
        bytes.extend(gzip.finish().unwrap());
        assert_eq!(
            decode_piece(&bytes, &Sha256::digest(b"content")).unwrap(),
            b"content"
        );
        assert!(decode_piece(&bytes, &[0; 32]).is_err());
        bytes[0] = 0;
        assert!(decode_piece(&bytes, &Sha256::digest(b"content")).is_err());
    }
    #[test]
    fn catalog_hosts_and_file_paths_fail_closed() {
        assert_eq!(
            normalize_cdn("http://jagex-akamai.aws.snxd.com/direct6/osrs-win/").unwrap(),
            format!("{CDN}/")
        );
        assert!(normalize_cdn("https://example.com/client").is_err());
        assert!(normalize_cdn("https://jagex.akamaized.net/client?x=1").is_err());
        for value in ["../outside", "C:/outside", "a\\outside", "/outside"] {
            assert!(safe_path(value).is_err());
        }
    }
    #[test]
    fn launch_handoff_uses_environment_and_keeps_the_companion_folder() {
        let install = Installed {
            id: "a".repeat(64),
            version: "test".into(),
            files: vec![],
        };
        let launch = launch_command(
            Path::new("data"),
            &install,
            &[("JX_SESSION_ID".into(), "session".into())],
            Some(Path::new("wine")),
        )
        .unwrap();
        assert_eq!(launch.get_program(), "wine");
        assert!(launch
            .get_args()
            .all(|a| !a.to_string_lossy().contains("session")));
        assert_eq!(
            launch.get_current_dir().unwrap(),
            Path::new("data").join("versions").join(&install.id)
        );
        assert!(launch
            .get_envs()
            .any(|(k, v)| k == "JX_ACCESS_TOKEN" && v.is_none()));
        let prefix = Path::new("data").join("wine-prefix");
        assert!(launch
            .get_envs()
            .any(|(k, v)| k == "WINEPREFIX" && v == Some(prefix.as_os_str())));
    }
    #[test]
    fn complete_install_detects_companion_corruption_and_rejects_bad_content() {
        let root =
            std::env::temp_dir().join(format!("cabbage-osrs-files-{:x}", rand::random::<u64>()));
        let id = "a".repeat(64);
        let folder = root.join("versions").join(&id);
        fs::create_dir_all(&folder).unwrap();
        let mut content = vec![0u8; 128];
        content[..2].copy_from_slice(b"MZ");
        content[60..64].copy_from_slice(&64u32.to_le_bytes());
        content[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0x86]);
        content.extend_from_slice(b"companion");
        let meta = Metafile {
            id: id.clone(),
            version: "test".into(),
            algorithm: "sha256".into(),
            pieces: Pieces {
                algorithm: "sha256".into(),
                digests: vec!["a".repeat(64)],
                hash_padding: false,
            },
            files: vec![
                FileEntry {
                    name: "osclient.exe".into(),
                    size: 128,
                },
                FileEntry {
                    name: "discord_game_sdk.dll".into(),
                    size: 9,
                },
            ],
        };
        let files = write_content(&folder, &meta, &content).unwrap();
        downloads::activate_json(
            &root,
            &Installed {
                id,
                version: "test".into(),
                files,
            },
        )
        .unwrap();
        assert!(installed(&root).is_some());
        let staging = root.join("bad-update");
        fs::create_dir(&staging).unwrap();
        assert!(write_content(&staging, &meta, &content[..128]).is_err());
        assert!(installed(&root).is_some());
        fs::write(folder.join("discord_game_sdk.dll"), b"corrupted").unwrap();
        assert!(installed(&root).is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "downloads current official OSRS publishing documents and chunks"]
    fn live_official_client_download_and_cached_reuse() {
        let root =
            std::env::temp_dir().join(format!("cabbage-osrs-download-{:x}", rand::random::<u64>()));
        let state = Mutex::new(Progress::default());
        let first = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert!(installed(&root).is_some());
        assert!(root
            .join("versions")
            .join(&first.id)
            .join("discord_game_sdk.dll")
            .exists());
        let second = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert_eq!(first.id, second.id);
        println!(
            "Official OSRS {} verified at {}",
            first.version,
            root.display()
        );
    }
}
