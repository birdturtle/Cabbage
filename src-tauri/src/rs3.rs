use crate::{
    clients::{configure_credentials, Progress},
    downloads,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

const CONFIG: &str = "https://rs.config.runescape.com/k=5/l=0/jav_config.ws?binaryType=4";
const BOOTSTRAP: &str = "https://files.publishing.production.jxp.jagex.com/v2/games/RuneScape.exe";
const OPENSSL: &str =
    "https://security.ubuntu.com/ubuntu/pool/main/o/openssl/libssl1.1_1.1.1f-1ubuntu2.24_amd64.deb";
const OPENSSL_HASH: &str = "7cf39d70a639017d1dd7c8d36daa2258063608688e449fddf40ffdd46f992a78";
const SSL_HASH: &str = "5ca16b9d9251a5d2c5d14db43ebbd81eab9799043a63a548aec75fb6b505c4f2";
const CRYPTO_HASH: &str = "bf99926de2ce739d3fdccc4a551d97d64aac6e968589aaad9dc4ac8d143b519a";
const MAX_FILE: usize = 64 * 1024 * 1024;

pub use cabbage_game_host::{Installed, GameConfig, installed};
use cabbage_game_host::{hash, executable, regular, runtime_valid};
#[cfg(test)]
use cabbage_game_host::startup_reply;
pub struct Prepared {
    pub install: Installed,
    config: Option<GameConfig>,
}
fn parse_config(bytes: &[u8]) -> Result<GameConfig, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = HashMap::new();
    let mut params = Vec::new();
    let mut keys = HashSet::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key == "param" {
                let (key, value) = value
                    .split_once('=')
                    .ok_or("Invalid RuneScape parameter.")?;
                if key.is_empty()
                    || !key.bytes().all(|b| b.is_ascii_digit())
                    || !keys.insert(key.to_owned())
                    || params.len() >= 128
                    || value.len() > 8192
                    || value.contains('\0')
                {
                    return Err("Invalid RuneScape parameter listing.".into());
                }
                params.push((key.into(), value.into()));
            } else if key != "msg" && fields.insert(key, value).is_some() {
                return Err("Duplicate RuneScape configuration field.".into());
            }
        }
    }
    if fields.get("binary_count") != Some(&"1")
        || fields.get("binary_name") != Some(&"rs2client")
        || fields.get("download_name_0") != Some(&"rs2client")
        || fields.get("launcher_version") != Some(&"224")
        || params.is_empty()
    {
        return Err("Unsupported RuneScape Linux client configuration.".into());
    }
    let codebase = fields
        .get("codebase")
        .ok_or("RuneScape download location is missing.")?
        .to_string();
    let url = url::Url::parse(&codebase).map_err(|_| "Invalid RuneScape download location.")?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !host
            .strip_prefix("world")
            .and_then(|h| h.strip_suffix(".runescape.com"))
            .is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_digit()))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/k=5/"
    {
        return Err("RuneScape download is outside the expected Jagex server.".into());
    }
    let crc = fields
        .get("download_crc_0")
        .ok_or("RuneScape checksum is missing.")?
        .parse()
        .map_err(|_| "Invalid RuneScape checksum.")?;
    let version = fields
        .get("server_version")
        .ok_or("RuneScape version is missing.")?
        .to_string();
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) || version.len() > 12 {
        return Err("Invalid RuneScape version.".into());
    }
    Ok(GameConfig {
        codebase,
        crc,
        version,
        params,
    })
}
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::other("Decompressed file exceeds its limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn decompress(bytes: &[u8], crc: u32) -> Result<Vec<u8>, String> {
    let mut out = Limited {
        bytes: Vec::new(),
        limit: MAX_FILE,
    };
    lzma_rs::lzma_decompress_with_options(
        &mut std::io::Cursor::new(bytes),
        &mut out,
        &lzma_rs::decompress::Options {
            memlimit: Some(MAX_FILE),
            ..Default::default()
        },
    )
    .map_err(|_| "RuneScape download is damaged or too large.")?;
    if crc32fast::hash(&out.bytes) != crc
        || out.bytes.get(..6) != Some(&[0x7f, b'E', b'L', b'F', 2, 1])
        || out.bytes.get(18..20) != Some(&[0x3e, 0])
    {
        return Err("RuneScape download did not match its checksum or Linux architecture.".into());
    }
    Ok(out.bytes)
}
async fn fetch(http: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = http.get(url).send().await.map_err(|_| {
        "Cannot reach the RuneScape download server. Check your connection and retry."
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "RuneScape download is unavailable (HTTP {}, {}). Retry later.",
            response.status().as_u16(),
            response.url().host_str().unwrap_or("Jagex")
        ));
    }
    downloads::read_limited(response, limit).await
}
fn trusted_download_url(url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    let world = host
        .strip_prefix("world")
        .and_then(|h| h.strip_suffix(".runescape.com"))
        .is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_digit()));
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && (world
            || matches!(
                host,
                "rs.config.runescape.com"
                    | "www.runescape.com"
                    | "runescape.com"
                    | "files.publishing.production.jxp.jagex.com"
                    | "security.ubuntu.com"
            ))
}
fn progress(state: &Mutex<Progress>, message: &str) {
    state.lock().unwrap().message = message.into();
}
fn activate(root: &Path, bytes: &[u8], version: &str, name: &str) -> Result<Installed, String> {
    let value = Installed {
        version: version.into(),
        hash: hash(bytes),
        executable: name.into(),
    };
    let folder = root.join("versions").join(&value.hash);
    fs::create_dir_all(&folder).map_err(|_| "Cannot create RuneScape installation directories.")?;
    let path = folder.join(name);
    if !regular(&path) || !fs::read(&path).is_ok_and(|b| hash(&b) == value.hash) {
        let staged = folder.join(format!("download-{:x}", rand::random::<u64>()));
        fs::write(&staged, bytes).map_err(|_| "Cannot save RuneScape client.")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot set RuneScape executable permissions.")?;
        }
        if path.exists() {
            fs::remove_file(&path).map_err(|_| "Cannot replace damaged RuneScape client.")?;
        }
        fs::rename(staged, path).map_err(|_| "Cannot activate RuneScape client.")?;
    }
    downloads::activate_json(root, &value)?;
    Ok(value)
}
fn extract_runtime(package: &[u8], root: &Path) -> Result<(), String> {
    if hash(package) != OPENSSL_HASH || !package.starts_with(b"!<arch>\n") {
        return Err("RuneScape compatibility runtime checksum did not match.".into());
    }
    let mut position = 8;
    let mut data = None;
    while position + 60 <= package.len() {
        let header = &package[position..position + 60];
        let size: usize = std::str::from_utf8(&header[48..58])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or("Invalid compatibility package header.")?;
        let end = position
            .checked_add(60)
            .and_then(|p| p.checked_add(size))
            .ok_or("Invalid compatibility package length.")?;
        let bytes = package
            .get(position + 60..end)
            .ok_or("Truncated compatibility package.")?;
        if header.starts_with(b"data.tar.xz") {
            if data.replace(bytes).is_some() {
                return Err("Ambiguous compatibility package.".into());
            }
        }
        position = end + size % 2;
    }
    let mut output = Limited {
        bytes: Vec::new(),
        limit: MAX_FILE,
    };
    lzma_rs::xz_decompress(
        &mut std::io::Cursor::new(data.ok_or("Compatibility package data is missing.")?),
        &mut output,
    )
    .map_err(|_| "Cannot decompress RuneScape compatibility runtime.")?;
    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).map_err(|_| "Cannot create RuneScape compatibility directory.")?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(output.bytes));
    let mut found = HashSet::new();
    for entry in archive
        .entries()
        .map_err(|_| "Invalid compatibility archive.")?
    {
        let mut entry = entry.map_err(|_| "Invalid compatibility archive entry.")?;
        let path = entry
            .path()
            .map_err(|_| "Invalid compatibility library path.")?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        let target = match path.as_str() {
            "usr/lib/x86_64-linux-gnu/libssl.so.1.1" => Some(("libssl.so.1.1", SSL_HASH)),
            "usr/lib/x86_64-linux-gnu/libcrypto.so.1.1" => Some(("libcrypto.so.1.1", CRYPTO_HASH)),
            "usr/share/doc/libssl1.1/copyright" => Some(("COPYRIGHT", "")),
            _ => None,
        };
        if let Some((name, expected)) = target {
            if !entry.header().entry_type().is_file()
                || !found.insert(name)
                || entry.size() > 8 * 1024 * 1024
            {
                return Err("Invalid compatibility library entry.".into());
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read compatibility library.")?;
            if !expected.is_empty() && hash(&bytes) != expected {
                return Err("Compatibility library checksum did not match.".into());
            }
            let staged = runtime.join(format!("{name}.next"));
            fs::write(&staged, bytes).map_err(|_| "Cannot save compatibility library.")?;
            fs::rename(staged, runtime.join(name))
                .map_err(|_| "Cannot activate compatibility library.")?;
        }
    }
    if !runtime_valid(root) || !found.contains("COPYRIGHT") {
        return Err("RuneScape compatibility libraries are missing.".into());
    }
    Ok(())
}
pub async fn ensure(root: &Path, state: &Mutex<Progress>) -> Result<Prepared, String> {
    fs::create_dir_all(root).map_err(|_| "Cannot create RuneScape client directory.")?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || !trusted_download_url(attempt.url()) {
                attempt.error("RuneScape redirect is outside the expected download servers")
            } else {
                attempt.follow()
            }
        }))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .user_agent("Cabbage/0.1")
        .build()
        .map_err(|_| "Cannot initialize RuneScape downloads.")?;
    if cfg!(windows) {
        progress(state, "Checking the RuneScape launcher...");
        let bytes = fetch(&http, BOOTSTRAP, MAX_FILE).await?;
        let version = verify_windows_bootstrap(root, &bytes)?;
        let install = activate(root, &bytes, &version, "RuneScape.exe")?;
        return Ok(Prepared {
            install,
            config: None,
        });
    }
    progress(state, "Checking RuneScape...");
    let config = parse_config(&fetch(&http, CONFIG, 256 * 1024).await?)?;
    let existing = installed(root).filter(|i| {
        i.executable == "rs2client"
            && fs::read(executable(root, i)).is_ok_and(|b| crc32fast::hash(&b) == config.crc)
    });
    let install = match existing {
        Some(value) => value,
        None => {
            progress(state, "Downloading RuneScape...");
            let url = format!(
                "{}client?binaryType=4&fileName=rs2client&crc={}",
                config.codebase, config.crc
            );
            let bytes = decompress(&fetch(&http, &url, MAX_FILE).await?, config.crc)?;
            activate(root, &bytes, &config.version, "rs2client")?
        }
    };
    if !runtime_valid(root) {
        progress(state, "Preparing RuneScape compatibility libraries...");
        extract_runtime(&fetch(&http, OPENSSL, 2 * 1024 * 1024).await?, root)?;
    }
    Ok(Prepared {
        install,
        config: Some(config),
    })
}
#[cfg(not(windows))]
fn verify_windows_bootstrap(_: &Path, _: &[u8]) -> Result<String, String> {
    Err("Windows verification requires Windows.".into())
}
#[cfg(windows)]
fn verify_windows_bootstrap(root: &Path, bytes: &[u8]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let path = root.join(format!("verify-{:x}.exe", rand::random::<u64>()));
    fs::write(&path, bytes).map_err(|_| "Cannot stage RuneScape signature verification.")?;
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", "$s=Get-AuthenticodeSignature -LiteralPath $env:CABBAGE_VERIFY_FILE; if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '(^|,\\s*)O=Jagex Limited(,|$)'){exit 1}; Write-Output (Get-Item -LiteralPath $env:CABBAGE_VERIFY_FILE).VersionInfo.FileVersion"])
        .env("CABBAGE_VERIFY_FILE", &path).creation_flags(0x08000000).stdout(Stdio::piped()).stderr(Stdio::null());
    configure_credentials(&mut command, &[]);
    let result = (|| {
        let mut child = command
            .spawn()
            .map_err(|_| "Cannot start Windows signature verification.")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        loop {
            if child
                .try_wait()
                .map_err(|_| "Cannot check Windows signature verification.")?
                .is_some()
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "Windows signature verification timed out. Check your connection and retry."
                        .to_string(),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let output = child
            .wait_with_output()
            .map_err(|_| "Cannot read Windows signature verification.")?;
        if !output.status.success() {
            return Err("RuneScape's Windows signature could not be verified. Check your connection and Windows certificate settings.".to_string());
        }
        let version = String::from_utf8(output.stdout)
            .map_err(|_| "Invalid RuneScape launcher version.")?
            .trim()
            .to_owned();
        if version.is_empty() || version.len() > 80 || version.chars().any(char::is_control) {
            return Err("Invalid RuneScape launcher version.".into());
        }
        Ok(version)
    })();
    let _ = fs::remove_file(path);
    result
}
pub fn spawn(
    root: &Path,
    prepared: Prepared,
    credentials: &[(String, String)],
    prefer_gpu: bool,
) -> Result<Child, String> {
    let root = fs::canonicalize(root).map_err(|_| "Cannot locate RuneScape installation.")?;
    let helper = crate::game_host::install(&root)?;
    let launch = root.join("launches");
    fs::create_dir_all(&launch).map_err(|_| "Cannot create RuneScape launch directory.")?;
    let handoff = launch.join(format!("{:x}.json", rand::random::<u64>()));
    let payload = serde_json::to_vec(&(prepared.install, prepared.config))
        .map_err(|_| "Cannot prepare RuneScape launch.")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&handoff)
        .and_then(|mut f| f.write_all(&payload))
        .map_err(|_| "Cannot save RuneScape launch preparation.")?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("launch.log"))
        .map_err(|_| "Cannot open RuneScape launch log.")?;
    let mut command = Command::new(helper);
    command
        .arg(&root)
        .arg(&handoff)
        .stdout(Stdio::piped())
        .stderr(log)
        .stdin(Stdio::null());
    configure_credentials(&mut command, credentials);
    crate::graphics::configure(&mut command, prefer_gpu);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    match command.spawn() {
        Ok(mut child) => {
            let output = child
                .stdout
                .take()
                .ok_or("Cannot read RuneScape startup status.")?;
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                let result = std::io::BufRead::read_line(
                    &mut std::io::BufReader::new(output.take(128)),
                    &mut line,
                );
                let _ = sender.send(result.is_ok() && line.trim() == "CABBAGE_RS3_READY");
            });
            if receiver.recv_timeout(Duration::from_secs(15)) != Ok(true) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(handoff);
                return Err("RuneScape could not start. Check its launch log and retry.".into());
            }
            Ok(child)
        }
        Err(_) => {
            let _ = fs::remove_file(handoff);
            Err("RuneScape could not start. Check the launch log and retry.".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_rejects_foreign_hosts_and_duplicate_parameters() {
        let bytes = include_bytes!("test-fixtures/rs3-linux-config.txt");
        let config = parse_config(bytes).unwrap();
        assert!(!config.params.is_empty());
        let text = String::from_utf8_lossy(bytes);
        assert!(parse_config(
            text.replace(&config.codebase, "https://example.com/k=5/")
                .as_bytes()
        )
        .is_err());
        assert!(parse_config(format!("{text}\nparam=1=duplicate").as_bytes()).is_err());
        assert!(parse_config(
            text.replace("binary_name=rs2client", "binary_name=../outside")
                .as_bytes()
        )
        .is_err());
    }
    #[test]
    fn redirects_and_compatibility_packages_fail_closed() {
        for value in [
            "http://world1.runescape.com/",
            "https://runescape.com.evil.test/",
            "https://evil.test/",
            "https://user@www.runescape.com/",
        ] {
            assert!(!trusted_download_url(&url::Url::parse(value).unwrap()));
        }
        assert!(trusted_download_url(
            &url::Url::parse("https://www.runescape.com/k=5/jav_config.ws?binaryType=4").unwrap()
        ));
        let root =
            std::env::temp_dir().join(format!("cabbage-rs3-invalid-{:x}", rand::random::<u64>()));
        assert!(extract_runtime(b"invalid package", &root).is_err());
        assert!(!root.exists());
    }
    #[test]
    #[cfg(windows)]
    fn unsigned_windows_bootstrap_is_rejected_and_staging_is_removed() {
        let root =
            std::env::temp_dir().join(format!("cabbage-rs3-signature-{:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        assert!(verify_windows_bootstrap(&root, b"not signed").is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn decompression_rejects_corruption_and_wrong_architecture() {
        let mut bytes = vec![0u8; 32];
        bytes[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        bytes[18..20].copy_from_slice(&[0x3e, 0]);
        let mut packed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(&bytes), &mut packed).unwrap();
        assert_eq!(decompress(&packed, crc32fast::hash(&bytes)).unwrap(), bytes);
        assert!(decompress(&packed, 0).is_err());
        assert!(decompress(b"bad", 0).is_err());
    }
    #[test]
    fn interrupted_update_keeps_current_installation_and_corruption_is_detected() {
        let root =
            std::env::temp_dir().join(format!("cabbage-rs3-install-{:x}", rand::random::<u64>()));
        let old = activate(&root, b"old", "old", "rs2client").unwrap();
        fs::create_dir_all(root.join("versions").join(hash(b"new")).join("rs2client")).unwrap();
        assert!(activate(&root, b"new", "new", "rs2client").is_err());
        assert_eq!(installed(&root).unwrap().hash, old.hash);
        fs::write(executable(&root, &old), b"bad").unwrap();
        assert!(installed(&root).is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn startup_protocol_keeps_paths_and_window_dimensions() {
        let reply = startup_reply(Path::new("/tmp/game data")).unwrap();
        assert_eq!(&reply[..4], &[0, 1, 0, 4]);
        assert!(reply.windows(14).any(|b| b == b"/tmp/game data"));
        assert_eq!(&reply[reply.len() - 4..], &[4, 0, 3, 0]);
    }
    #[test]
    #[ignore = "downloads current RS3 files and private compatibility runtime"]
    fn live_rs3_install_and_reuse() {
        let root =
            std::env::temp_dir().join(format!("cabbage-rs3-download-{:x}", rand::random::<u64>()));
        let state = Mutex::new(Progress::default());
        let first = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert!(installed(&root).is_some());
        let second = tauri::async_runtime::block_on(ensure(&root, &state)).unwrap();
        assert_eq!(first.install.hash, second.install.hash);
        println!(
            "RuneScape {} downloaded at {}",
            first.install.version,
            root.display()
        );
    }
}
