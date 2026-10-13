//! Standalone replacement worker. No account or game access.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize)]
pub struct Plan {
    pub target: PathBuf,
    pub staged: PathBuf,
    pub sha256: String,
    pub parent: u32,
    pub version: String,
    pub token: String,
}
pub fn companion(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}
fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}
fn digest(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = fs::File::open(path).map_err(|_| "Cannot read update.")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "Cannot read update.")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn validate(plan: &Plan) -> Result<(), String> {
    if !plan.target.is_absolute()
        || !plan.staged.is_absolute()
        || plan.target.parent() != plan.staged.parent()
        || plan.target == plan.staged
        || !regular(&plan.target)
        || !regular(&plan.staged)
        || plan.sha256.len() != 64
        || plan.token.len() != 64
        || !plan.token.bytes().all(|b| b.is_ascii_hexdigit())
        || digest(&plan.staged)? != plan.sha256
    {
        return Err("Update files failed verification.".into());
    }
    for path in plan.target.ancestors().skip(1) {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Update path must not be a symbolic link.".into());
        }
    }
    Ok(())
}
fn wait_parent(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, WaitForSingleObject},
        };
        let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
        if handle.is_null() {
            // The launcher can exit before the worker obtains its handle.
            if unsafe { windows_sys::Win32::Foundation::GetLastError() } == 87 {
                return Ok(());
            }
            return Err("Cannot track launcher exit.".into());
        }
        let result = unsafe { WaitForSingleObject(handle, 60000) };
        unsafe {
            CloseHandle(handle);
        }
        if result != 0 {
            return Err("Launcher did not close. Retry the update.".into());
        }
    }
    #[cfg(unix)]
    {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if unsafe { libc::kill(pid as i32, 0) } != 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Launcher did not close. Retry the update.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}
fn launch(plan: &Plan, ack: Option<&Path>) -> Result<std::process::Child, String> {
    let mut command = Command::new(&plan.target);
    command
        .current_dir(plan.target.parent().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in [
        "APPIMAGE",
        "APPDIR",
        "ARGV0",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "CABBAGE_UPDATE_ACK",
        "CABBAGE_UPDATE_TOKEN",
        "CABBAGE_UPDATE_VERSION",
        "CABBAGE_UPDATE_TARGET",
    ] {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("JX_") {
            command.env_remove(name);
        }
    }
    if let Some(ack) = ack {
        command
            .env("CABBAGE_UPDATE_ACK", ack)
            .env("CABBAGE_UPDATE_TOKEN", &plan.token)
            .env("CABBAGE_UPDATE_VERSION", &plan.version)
            .env("CABBAGE_UPDATE_TARGET", &plan.target);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|_| "Cannot restart Cabbage.".to_string())
}
fn replace(plan: &Plan) -> Result<(), String> {
    validate(plan)?;
    let backup = plan.target.with_extension("previous");
    let ack = companion(&plan.staged, "ack");
    if backup.exists() && !regular(&backup) {
        return Err("Cannot keep previous launcher.".into());
    }
    if backup.exists() {
        fs::remove_file(&backup).map_err(|_| "Cannot replace previous launcher backup.")?;
    }
    if fs::rename(&plan.target, &backup).is_err() {
        let _ = launch(plan, None);
        return Err("Cannot replace launcher. Close other Cabbage windows and retry.".into());
    }
    if fs::rename(&plan.staged, &plan.target).is_err() {
        fs::rename(&backup, &plan.target)
            .map_err(|_| "Cannot restore launcher; use the .previous backup.")?;
        let _ = launch(plan, None);
        return Err("Cannot activate update. Previous launcher restored.".into());
    }
    let result = (|| {
        let mut child = launch(plan, Some(&ack))?;
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if fs::read(&ack).is_ok_and(|bytes| bytes == plan.token.as_bytes()) {
                return Ok(());
            }
            if child
                .try_wait()
                .map_err(|_| "Cannot track updated launcher.")?
                .is_some()
                || Instant::now() >= deadline
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Updated launcher did not start. Previous version restored.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })();
    let _ = fs::remove_file(&ack);
    if result.is_err() {
        fs::remove_file(&plan.target)
            .map_err(|_| "Cannot remove failed update; use the .previous backup.")?;
        fs::rename(&backup, &plan.target)
            .map_err(|_| "Cannot restore launcher; use the .previous backup.")?;
        // Record the error before reopening the previous version.
        let _ = fs::write(
            plan.target.with_extension("update-error"),
            result.as_ref().unwrap_err(),
        );
        let _ = launch(plan, None);
    }
    result
}
pub fn run(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|_| "Cannot read update plan.")?;
    if bytes.len() > 16384 {
        return Err("Invalid update plan.".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|_| "Invalid update plan.")?;
    validate(&plan)?;
    let ready = companion(&plan.staged, "ready");
    fs::write(&ready, &plan.token).map_err(|_| "Cannot acknowledge update readiness.")?;
    let waiting = wait_parent(plan.parent);
    let _ = fs::remove_file(&ready);
    waiting?;
    let result = replace(&plan);
    if let Err(error) = &result {
        let _ = fs::write(plan.target.with_extension("update-error"), error);
    }
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(&plan.staged);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path, name: &str, mode: &str) -> PathBuf {
        let source = root.join(format!("{name}.rs"));
        let binary = root.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        let body = match mode {
            "ack" => "let path=std::env::var_os(\"CABBAGE_UPDATE_ACK\").unwrap(); let token=std::env::var(\"CABBAGE_UPDATE_TOKEN\").unwrap(); std::fs::write(path,token).unwrap();".to_owned(),
            "fail" => "std::process::exit(1);".to_owned(),
            "parent" => "std::thread::sleep(std::time::Duration::from_secs(1));".to_owned(),
            _ => format!("std::fs::write({:?},b\"restored\").unwrap();", root.join("restored").to_str().unwrap()),
        };
        fs::write(&source, format!("fn main() {{ {body} }}")).unwrap();
        assert!(Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success());
        binary
    }
    #[test]
    fn successful_restart_keeps_backup_and_failed_restart_restores_previous() {
        for mode in ["ack", "fail"] {
            let root = std::env::temp_dir()
                .join(format!("cabbage replacement {} {mode}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let target = fixture(&root, "old", "old");
            let old = fs::read(&target).unwrap();
            let staged = fixture(&root, "new", mode);
            let new = fs::read(&staged).unwrap();
            let plan = Plan {
                target: target.clone(),
                sha256: digest(&staged).unwrap(),
                staged,
                parent: 0,
                version: "test".into(),
                token: "a".repeat(64),
            };
            let result = replace(&plan);
            if mode == "ack" {
                assert!(result.is_ok());
                assert_eq!(fs::read(&target).unwrap(), new);
                assert_eq!(fs::read(target.with_extension("previous")).unwrap(), old);
            } else {
                assert!(result.is_err());
                assert_eq!(fs::read(&target).unwrap(), old);
                let deadline = Instant::now() + Duration::from_secs(10);
                while !root.join("restored").exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
                assert_eq!(fs::read(root.join("restored")).unwrap(), b"restored");
                assert!(target.with_extension("update-error").exists());
            }
            // Allow the short-lived fixture process to finish on Windows.
            for _ in 0..50 {
                if fs::remove_dir_all(&root).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(!root.exists());
        }
    }
    #[test]
    fn worker_acknowledges_readiness_and_waits_for_parent_before_replacement() {
        let root = std::env::temp_dir().join(format!("cabbage parent wait {}", std::process::id())); fs::create_dir_all(&root).unwrap();
        let target = fixture(&root,"old","old"); let old = fs::read(&target).unwrap();
        let staged = fixture(&root,"new","ack"); let parent = fixture(&root,"parent","parent");
        let mut child = Command::new(parent).spawn().unwrap();
        let plan = Plan { target:target.clone(), sha256:digest(&staged).unwrap(), staged:staged.clone(), parent:child.id(), version:"test".into(), token:"a".repeat(64) };
        let path = companion(&staged,"plan"); fs::write(&path,serde_json::to_vec(&plan).unwrap()).unwrap();
        let worker = std::thread::spawn(move || run(&path));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !companion(&staged,"ready").exists() && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
        assert_eq!(fs::read(companion(&staged,"ready")).unwrap(),plan.token.as_bytes());
        assert_eq!(fs::read(&target).unwrap(),old);
        child.wait().unwrap(); worker.join().unwrap().unwrap();
        assert_eq!(fs::read(target.with_extension("previous")).unwrap(),old);
        assert!(!companion(&staged,"plan").exists()); assert!(!companion(&staged,"ready").exists());
        for _ in 0..50 { if fs::remove_dir_all(&root).is_ok() { break; } std::thread::sleep(Duration::from_millis(20)); }
        assert!(!root.exists());
    }
    #[test]
    fn corrupt_update_preserves_launcher() {
        let root = std::env::temp_dir().join(format!("cabbage-update-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("Cabbage");
        let staged = root.join("next");
        fs::write(&target, b"old").unwrap();
        fs::write(&staged, b"bad").unwrap();
        let plan = Plan {
            target: target.clone(),
            staged,
            sha256: "0".repeat(64),
            parent: 0,
            version: "test".into(),
            token: "a".repeat(64),
        };
        assert!(replace(&plan).is_err());
        assert_eq!(fs::read(target).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
}
