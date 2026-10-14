use super::*;
use cabbage_dependency_helper::{Client, Phase, Plan, Report};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const HELPER: &[u8] = include_bytes!(env!("CABBAGE_DEPENDENCY_HELPER"));
pub fn client(id: crate::clients::ClientId) -> Client {
    match id {
        crate::clients::ClientId::Runelite => Client::Runelite,
        crate::clients::ClientId::OfficialOsrs => Client::OfficialOsrs,
        crate::clients::ClientId::OfficialRs3 => Client::OfficialRs3,
    }
}
pub fn probe() -> Vec<Plan> {
    [Client::Runelite, Client::OfficialOsrs, Client::OfficialRs3]
        .into_iter()
        .map(cabbage_dependency_helper::probe)
        .collect()
}
fn terminal(helper: &Path, client: Client, report: &Path, token: &str) -> Result<Command, String> {
    let candidates: [(&str, &[&str]); 7] = [
        ("/usr/bin/konsole", &["--separate", "-e"]),
        ("/usr/bin/gnome-terminal", &["--wait", "--"]),
        (
            "/usr/bin/xfce4-terminal",
            &["--disable-server", "--execute"],
        ),
        ("/usr/bin/kitty", &["--"]),
        ("/usr/bin/alacritty", &["-e"]),
        ("/usr/bin/foot", &["--"]),
        ("/usr/bin/xterm", &["-e"]),
    ];
    let (program, args) = candidates
        .into_iter()
        .find(|(path, _)| Path::new(path).is_file())
        .ok_or("Install a terminal emulator, such as Konsole, to open dependency setup.")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(helper)
        .arg(match client {
            Client::Runelite => "runelite",
            Client::OfficialOsrs => "officialOsrs",
            Client::OfficialRs3 => "officialRs3",
        })
        .arg(report)
        .arg(token);
    crate::clients::configure_credentials(&mut command, &[]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("JX_") {
            command.env_remove(name);
        }
    }
    for name in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "APPIMAGE",
        "APPDIR",
        "ARGV0",
    ] {
        command.env_remove(name);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}
pub fn install(
    id: crate::clients::ClientId,
    root: &Path,
    state: &Shared,
) -> Result<String, String> {
    let client = client(id);
    let plan = cabbage_dependency_helper::probe(client);
    if !plan.supported {
        return Err("Automatic dependency setup currently supports Arch and CachyOS x64. Install the missing requirements with your package manager, then select Check again.".into());
    }
    if plan.missing.is_empty() {
        return Ok("Requirements are already installed.".into());
    }
    if !Path::new("/usr/bin/sudo").is_file() {
        return Err("Install sudo before using terminal dependency setup.".into());
    }
    let helper = crate::game_host::install_named(root, HELPER, "cabbage-dependency-helper")?;
    fs::create_dir_all(root).map_err(|_| "Cannot create setup directory.")?;
    let folder = root.join(format!("session-{:x}", rand::random::<u64>()));
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&folder)
        .map_err(|_| "Cannot create private setup session.")?;
    let report_path = folder.join("status.json");
    let token = format!(
        "{:032x}{:032x}",
        rand::random::<u128>(),
        rand::random::<u128>()
    );
    let result = (|| {
        let mut child = terminal(&helper, client, &report_path, &token)?
            .spawn()
            .map_err(|_| "Cannot open dependency setup terminal.")?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut started = false;
        loop {
            if let Ok(bytes) = fs::read(&report_path) {
                if bytes.len() > 16384 {
                    return Err("Invalid dependency setup response.".into());
                }
                let report: Report = serde_json::from_slice(&bytes)
                    .map_err(|_| "Invalid dependency setup response.")?;
                if report.token != token {
                    return Err("Dependency setup response does not match this session.".into());
                }
                started = true;
                state.lock().unwrap().message = report.message.clone();
                match report.phase {
                    Phase::Finished | Phase::Cancelled => return Ok(report.message),
                    Phase::Failed => return Err(report.message),
                    Phase::Waiting | Phase::Installing => {
                        let running = fs::read_link(format!("/proc/{}/exe", report.pid))
                            .ok()
                            .and_then(|p| fs::canonicalize(p).ok())
                            == fs::canonicalize(&helper).ok();
                        if !running {
                            return Err("Setup terminal closed before completion. Finish any package transaction, then select Check again.".into());
                        }
                    }
                }
            }
            if !started
                && (Instant::now() >= deadline
                    || child
                        .try_wait()
                        .map_err(|_| "Cannot track setup terminal.")?
                        .is_some())
            {
                return Err("The setup terminal could not start. Retry or choose another terminal emulator.".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    })();
    let _ = fs::remove_dir_all(folder);
    result
}
