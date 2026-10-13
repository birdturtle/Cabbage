use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::time::Duration;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
const SSL_HASH: &str = "5ca16b9d9251a5d2c5d14db43ebbd81eab9799043a63a548aec75fb6b505c4f2";
const CRYPTO_HASH: &str = "bf99926de2ce739d3fdccc4a551d97d64aac6e968589aaad9dc4ac8d143b519a";
const MAX_FILE: usize = 64 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub hash: String,
    pub executable: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub codebase: String,
    pub crc: u32,
    pub version: String,
    pub params: Vec<(String, String)>,
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn executable(root: &Path, install: &Installed) -> PathBuf {
    root.join("versions")
        .join(&install.hash)
        .join(&install.executable)
}
pub fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}
pub fn installed(root: &Path) -> Option<Installed> {
    let value: Installed =
        serde_json::from_slice(&fs::read(root.join("current.json")).ok()?).ok()?;
    if !valid_hash(&value.hash)
        || !matches!(value.executable.as_str(), "rs2client" | "RuneScape.exe")
    {
        return None;
    }
    let folder = root.join("versions").join(&value.hash);
    if fs::symlink_metadata(&folder).ok()?.file_type().is_symlink() {
        return None;
    }
    let path = executable(root, &value);
    if !regular(&path)
        || fs::metadata(&path).ok()?.len() > MAX_FILE as u64
        || hash(&fs::read(path).ok()?) != value.hash
    {
        return None;
    }
    Some(value)
}
pub fn runtime_valid(root: &Path) -> bool {
    [
        ("libssl.so.1.1", SSL_HASH),
        ("libcrypto.so.1.1", CRYPTO_HASH),
    ]
    .iter()
    .all(|(name, expected)| {
        let path = root.join("runtime").join(name);
        regular(&path)
            && fs::metadata(&path).is_ok_and(|m| m.len() <= 8 * 1024 * 1024)
            && fs::read(path).is_ok_and(|b| hash(&b) == *expected)
    })
}
pub fn host(root: &Path, handoff: &Path) -> Result<i32, String> {
    if !regular(handoff) || !fs::metadata(handoff).is_ok_and(|m| m.len() <= 2 * 1024 * 1024) {
        return Err("Invalid RuneScape launch preparation.".into());
    }
    let bytes = fs::read(handoff).map_err(|_| "Cannot read RuneScape launch preparation.")?;
    let _ = fs::remove_file(handoff);
    let (install, config): (Installed, Option<GameConfig>) =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid RuneScape launch preparation.")?;
    let current =
        installed(root).ok_or("RuneScape files are damaged. Retry Play to repair them.")?;
    if current.hash != install.hash || current.executable != install.executable {
        return Err("RuneScape changed during launch. Retry Play.".into());
    }
    #[cfg(target_os = "linux")]
    {
        linux::run(
            root,
            &install,
            &config.ok_or("RuneScape configuration is missing.")?,
        )
    }
    #[cfg(windows)]
    {
        let _ = config;
        windows::run(root, &install)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = config;
        Err("RuneScape is unsupported on this platform.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_malformed_handoff_do_not_launch() {
        let root = std::env::temp_dir().join(format!("cabbage-host-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let handoff = root.join("handoff.json");
        assert!(host(&root, &handoff).is_err());
        fs::write(&handoff, b"invalid").unwrap();
        assert!(host(&root, &handoff).is_err());
        assert!(!handoff.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn startup_protocol_retains_version_and_path_validation() {
        assert_eq!(
            &startup_reply(Path::new("/tmp/data")).unwrap()[..4],
            &[0, 1, 0, 4]
        );
        assert!(startup_reply(Path::new("bad\0path")).is_err());
    }
}

pub fn startup_reply(folder: &Path) -> Result<Vec<u8>, String> {
    let folder = folder
        .to_str()
        .ok_or("RuneScape data path must be valid UTF-8.")?
        .as_bytes();
    if folder.contains(&0) || folder.len() > 8000 {
        return Err("Invalid RuneScape data path.".into());
    }
    let mut out = vec![0, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(folder);
    out.push(0);
    out.extend_from_slice(folder);
    out.push(0);
    out.extend_from_slice(&[0, 4, 0, 0, 7]);
    out.extend_from_slice(b"RuneScape\0");
    for value in [765u16, 540, 3840, 2160, 1024, 768] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    Ok(out)
}
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::ffi::CString;
    struct Pipes(Vec<PathBuf>);
    impl Drop for Pipes {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = fs::remove_file(path);
            }
        }
    }
    fn make_pipes(pid: u32) -> Result<Pipes, String> {
        let mut pipes = Pipes(Vec::new());
        for suffix in ["i", "o"] {
            let path = PathBuf::from(format!("/tmp/RS2LauncherConnection_{pid}_{suffix}"));
            let name = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } != 0 {
                return Err("Cannot create RuneScape's private launcher connection.".into());
            }
            pipes.0.push(path);
        }
        Ok(pipes)
    }
    fn protocol(input: PathBuf, output: PathBuf, init: Vec<u8>) -> std::io::Result<()> {
        let mut input = fs::File::open(input)?;
        let mut output = fs::OpenOptions::new().write(true).open(output)?;
        loop {
            let mut length = [0; 2];
            if input.read_exact(&mut length).is_err() {
                return Ok(());
            }
            let length = u16::from_le_bytes(length) as usize;
            if !(2..=8192).contains(&length) {
                return Err(std::io::Error::other("Invalid RuneScape message size"));
            }
            let mut msg = vec![0; length];
            input.read_exact(&mut msg)?;
            let reply = match msg[..2] {
                [0, 0] => Some(init.as_slice()),
                [0, 32] => Some(&[0, 11, 0, 1][..]),
                [0, 3] => {
                    eprintln!("RuneScape reached the login screen.");
                    None
                }
                _ => None,
            };
            if let Some(reply) = reply {
                output.write_all(&(reply.len() as u16).to_le_bytes())?;
                output.write_all(reply)?;
            }
        }
    }
    pub fn run(root: &Path, install: &Installed, config: &GameConfig) -> Result<i32, String> {
        if !runtime_valid(root) {
            return Err("RuneScape compatibility libraries are damaged. Retry Play.".into());
        }
        for name in ["libSDL2-2.0.so.0", "libEGL.so.1", "libOpenGL.so.0"] {
            let name_c = CString::new(name).unwrap();
            let handle =
                unsafe { libc::dlopen(name_c.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
            if handle.is_null() {
                return Err(format!("RuneScape requires {name}. Install your distribution's SDL2 and OpenGL runtime packages."));
            }
            unsafe {
                libc::dlclose(handle);
            }
        }
        supervise(root, install, config)
    }
    fn supervise(root: &Path, install: &Installed, config: &GameConfig) -> Result<i32, String> {
        let pid = std::process::id();
        let pipes = make_pipes(pid)?;
        let data = root.join("data");
        fs::create_dir_all(&data).map_err(|_| "Cannot create RuneScape data directory.")?;
        let init = startup_reply(&data)?;
        let input = pipes.0[0].clone();
        let output = pipes.0[1].clone();
        std::thread::spawn(move || {
            if protocol(input, output, init).is_err() {
                eprintln!("RuneScape launcher communication ended.");
            }
        });
        let mut command = Command::new(executable(root, install));
        for (key, value) in &config.params {
            command.arg(key).arg(value);
        }
        command
            .arg("launcher")
            .arg(pid.to_string())
            .current_dir(&data)
            .env("LD_LIBRARY_PATH", root.join("runtime"));
        // SDL must use the X11 route supported by this version of the native game.
        command.env("SDL_VIDEODRIVER", "x11");
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("launch.log"))
            .map_err(|_| "Cannot open RuneScape launch log.")?;
        command
            .stdout(
                log.try_clone()
                    .map_err(|_| "Cannot open RuneScape launch log.")?,
            )
            .stderr(log);
        let mut child = command
            .spawn()
            .map_err(|_| "RuneScape could not start. Check the launch log.")?;
        println!("CABBAGE_RS3_READY");
        let _ = std::io::stdout().flush();
        let exit = child
            .wait()
            .map_err(|_| "Cannot track the RuneScape process.")?;
        Ok(exit.code().unwrap_or(1))
    }
    #[test]
    fn supervisor_waits_for_game_exit_and_cleans_up_pipes() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("cabbage-supervisor-{}", std::process::id()));
        let install = Installed {
            version: "test".into(),
            hash: hash(b"fixture"),
            executable: "rs2client".into(),
        };
        let file = executable(&root, &install);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"#!/bin/sh\nsleep 1\nexit 7\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        let config = GameConfig {
            codebase: String::new(),
            crc: 0,
            version: "test".into(),
            params: vec![],
        };
        let started = std::time::Instant::now();
        assert_eq!(supervise(&root, &install, &config).unwrap(), 7);
        assert!(started.elapsed() >= std::time::Duration::from_millis(900));
        for suffix in ["i", "o"] {
            assert!(!Path::new(&format!(
                "/tmp/RS2LauncherConnection_{}_{suffix}",
                std::process::id()
            ))
            .exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
                QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            },
            Threading::GetCurrentProcess,
        },
    };
    pub fn run(root: &Path, install: &Installed) -> Result<i32, String> {
        use std::os::windows::process::CommandExt;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err("Cannot create RuneScape process tracking.".into());
            }
            if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
                CloseHandle(job);
                return Err("Cannot track RuneScape child processes.".into());
            }
            let log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("launch.log"))
                .map_err(|_| "Cannot open RuneScape launch log.")?;
            let mut child = match Command::new(executable(root, install))
                .current_dir(root)
                .creation_flags(0x08000000)
                .stdout(
                    log.try_clone()
                        .map_err(|_| "Cannot open RuneScape launch log.")?,
                )
                .stderr(log)
                .spawn()
            {
                Ok(value) => value,
                Err(_) => {
                    CloseHandle(job);
                    return Err("RuneScape could not start.".into());
                }
            };
            println!("CABBAGE_RS3_READY");
            let _ = std::io::stdout().flush();
            let result = child
                .wait()
                .map_err(|_| "Cannot track RuneScape launcher.")?;
            loop {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                if QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of_val(&info) as u32,
                    std::ptr::null_mut(),
                ) == 0
                {
                    CloseHandle(job);
                    return Err("Cannot check RuneScape process lifetime.".into());
                }
                if info.ActiveProcesses <= 1 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            // No kill-on-close flag: closing Cabbage never terminates a game.
            CloseHandle(job);
            Ok(result.code().unwrap_or(1))
        }
    }
}
