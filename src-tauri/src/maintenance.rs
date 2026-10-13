use crate::{
    clients::{ClientId, Progress},
    downloads, osrs, rs3, runelite,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Repair,
    Uninstall,
}
fn paths(client: ClientId) -> &'static [&'static str] {
    match client {
        ClientId::Runelite => &["launchers", "runtimes", "current.json"],
        ClientId::OfficialOsrs => &["versions", "wine-version.txt", "current.json"],
        ClientId::OfficialRs3 => &["versions", "runtime", "current.json"],
    }
}
pub fn has_files(root: &Path, client: ClientId) -> bool {
    paths(client)
        .iter()
        .any(|name| fs::symlink_metadata(root.join(name)).is_ok())
}
fn regular_directory(path: &Path) -> Result<(), String> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("Client directory must be a regular directory.".into());
        }
    }
    Ok(())
}
fn remove(path: &Path) -> Result<(), String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Cannot inspect client files.".into()),
    };
    if meta.file_type().is_symlink() {
        fs::remove_file(path)
    } else if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|_| "Cannot remove client files. Close the game and retry.".into())
}

// Check independently of the GUI manager, since games can outlive a launcher restart.
#[cfg(target_os = "linux")]
fn managed_argument(arg: &[u8], root: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let bytes = root.as_os_str().as_bytes();
    if arg == bytes || (arg.starts_with(bytes) && arg.get(bytes.len()) == Some(&b'/')) {
        return true;
    }
    // Wine can expose argv through its Z: mapping instead of the original Unix path.
    let wine = format!("Z:{}", root.to_string_lossy().replace('/', "\\"));
    let wine = wine.as_bytes();
    arg.get(..wine.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(wine))
        && (arg.len() == wine.len() || arg.get(wine.len()) == Some(&b'\\'))
}
pub fn ensure_closed(root: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let processes =
            fs::read_dir("/proc").map_err(|_| "Cannot check running game processes.")?;
        for entry in processes.flatten() {
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            if fs::read_link(entry.path().join("exe")).is_ok_and(|exe| exe.starts_with(&canonical))
            {
                return Err("Close this client before repairing or uninstalling it.".into());
            }
            if let Ok(args) = fs::read(entry.path().join("cmdline")) {
                if args
                    .split(|b| *b == 0)
                    .any(|arg| managed_argument(arg, &canonical) || managed_argument(arg, root))
                {
                    return Err("Close this client before repairing or uninstalling it.".into());
                }
            }
        }
    }
    #[cfg(windows)]
    {
        use std::{
            os::windows::process::CommandExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut command = Command::new("powershell.exe");
        let root_text = root.to_string_lossy().replace('/', "\\");
        let root_text = root_text.strip_prefix(r"\\?\").unwrap_or(&root_text);
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$p=$env:CABBAGE_CLIENT_ROOT; $found=Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($p,[StringComparison]::OrdinalIgnoreCase)) -or ($_.CommandLine -and $_.CommandLine.IndexOf($p,[StringComparison]::OrdinalIgnoreCase) -ge 0) }; if($found){exit 2}"])
            .env("CABBAGE_CLIENT_ROOT", root_text).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000);
        crate::clients::configure_credentials(&mut command, &[]);
        let mut child = command
            .spawn()
            .map_err(|_| "Cannot check running game processes.")?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(exit) = child
                .try_wait()
                .map_err(|_| "Cannot check running game processes.")?
            {
                return match exit.code() {
                    Some(0) => Ok(()),
                    Some(2) => Err("Close this client before repairing or uninstalling it.".into()),
                    _ => Err("Cannot check running game processes. Try again.".into()),
                };
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Checking running games timed out. Try again.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = root;
        return Err("Client maintenance is unsupported on this platform.".into());
    }
    #[cfg(target_os = "linux")]
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    existed: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    client: ClientId,
    entries: Vec<Entry>,
}
fn transaction(root: &Path) -> PathBuf {
    root.join(".maintenance")
}
pub fn recover(root: &Path, client: ClientId) -> Result<(), String> {
    regular_directory(root)?;
    let folder = transaction(root);
    if !folder.exists() {
        return Ok(());
    }
    regular_directory(&folder)?;
    let record = folder.join("current.json");
    // The record is saved before any active files are moved.
    if !record.exists() {
        if fs::read_dir(folder.join("files")).is_ok_and(|mut files| files.next().is_some()) {
            return Err(
                "Client backup is missing its recovery record. Existing files were kept.".into(),
            );
        }
        return remove(&folder);
    }
    let journal: Journal = serde_json::from_slice(
        &fs::read(&record).map_err(|_| "Cannot read interrupted client maintenance.")?,
    )
    .map_err(|_| "Cannot recover client maintenance state.")?;
    if journal.client != client
        || journal.entries.len() != paths(client).len()
        || journal
            .entries
            .iter()
            .zip(paths(client))
            .any(|(entry, name)| entry.name != *name)
    {
        return Err("Invalid client maintenance paths.".into());
    }
    ensure_closed(root)?;
    let saved = folder.join("files");
    regular_directory(&saved)?;
    if !saved.is_dir() {
        return Err("Client recovery backup is missing.".into());
    }
    if !folder.join("committed").is_file() {
        // Restore binaries before the active record, which is always the last entry.
        for entry in &journal.entries {
            let backup = saved.join(&entry.name);
            let active = root.join(&entry.name);
            if entry.existed && fs::symlink_metadata(&backup).is_ok() {
                remove(&active)?;
                fs::rename(backup, active).map_err(|_| {
                    "Cannot restore the previous client installation. Retry before launching."
                })?;
            } else if !entry.existed {
                remove(&active)?;
            }
        }
    }
    remove(&folder)
}
fn activate(root: &Path, fresh: Option<&Path>, client: ClientId) -> Result<(), String> {
    let folder = transaction(root);
    fs::create_dir(&folder).map_err(|_| "Cannot prepare client replacement.")?;
    let saved = folder.join("files");
    fs::create_dir(&saved).map_err(|_| "Cannot prepare client backup.")?;
    let journal = Journal {
        client,
        entries: paths(client)
            .iter()
            .map(|name| Entry {
                name: (*name).into(),
                existed: fs::symlink_metadata(root.join(name)).is_ok(),
            })
            .collect(),
    };
    downloads::activate_json(&folder, &journal)?;
    let result = (|| {
        for entry in &journal.entries {
            let active = root.join(&entry.name);
            if entry.existed {
                fs::rename(&active, saved.join(&entry.name))
                    .map_err(|_| "Cannot replace client files. Close the game and retry.")?;
            }
            if let Some(fresh) = fresh {
                let new = fresh.join(&entry.name);
                if fs::symlink_metadata(&new).is_ok() {
                    fs::rename(new, active)
                        .map_err(|_| "Cannot activate repaired client files.")?;
                }
            }
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join("committed"))
            .map_err(|_| "Cannot commit client replacement.")?;
        Ok(())
    })();
    match result {
        Err(error) => {
            recover(root, client).map_err(|_| {
                "Client replacement failed and recovery is incomplete. Retry before launching."
                    .to_string()
            })?;
            Err(error)
        }
        Ok(()) => {
            // Cleanup can be retried on the next operation; committed files are already active.
            let _ = remove(&folder);
            Ok(())
        }
    }
}

pub async fn run(
    clients_root: &Path,
    client: ClientId,
    operation: Operation,
    state: &Mutex<Progress>,
) -> Result<(), String> {
    regular_directory(clients_root)?;
    let root = clients_root.join(client.directory());
    regular_directory(&root)?;
    fs::create_dir_all(&root).map_err(|_| "Cannot create the client directory.")?;
    recover(&root, client)?;
    ensure_closed(&root)?;
    let staging = root.join(".repair");
    regular_directory(&staging)?;
    remove(&staging)?;
    if operation == Operation::Uninstall {
        return activate(&root, None, client);
    }
    if let Some(reason) = client.unavailable_reason(std::env::consts::OS, std::env::consts::ARCH) {
        return Err(reason.into());
    }
    fs::create_dir(&staging).map_err(|_| "Cannot create repair staging.")?;
    let result = async {
        // Prepare only managed binaries. Existing game settings, Wine prefix, and caches stay in place.
        match client {
            ClientId::Runelite => {
                runelite::ensure(&staging, state).await?;
            }
            ClientId::OfficialOsrs => {
                osrs::ensure(&staging, state).await?;
            }
            ClientId::OfficialRs3 => {
                rs3::ensure(&staging, state).await?;
            }
        }
        ensure_closed(&root)?;
        state.lock().unwrap().message = "Replacing client files...".into();
        activate(&root, Some(&staging), client)
    }
    .await;
    let _ = remove(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "linux")]
    fn wine_and_unix_paths_match_only_the_selected_client() {
        let root = Path::new("/tmp/client files");
        assert!(managed_argument(b"/tmp/client files/versions/game", root));
        assert!(managed_argument(
            br"Z:\tmp\client files\versions\osclient.exe",
            root
        ));
        assert!(!managed_argument(
            b"/tmp/client files-other/versions/game",
            root
        ));
        assert!(!managed_argument(b"/tmp/other-client/game", root));
    }
    fn root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("cabbage maintenance {:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        root
    }
    fn put(root: &Path, name: &str, bytes: &[u8]) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn uninstall_removes_only_client_artifacts_and_preserves_game_data() {
        for client in [
            ClientId::Runelite,
            ClientId::OfficialOsrs,
            ClientId::OfficialRs3,
        ] {
            let root = root();
            for name in paths(client) {
                put(&root, name, b"managed");
            }
            for name in [
                "data/settings",
                "wine-prefix/user.reg",
                "accounts.json",
                "launch.log",
                "cache/file",
            ] {
                put(&root, name, b"keep");
            }
            activate(&root, None, client).unwrap();
            assert!(!has_files(&root, client));
            for name in [
                "data/settings",
                "wine-prefix/user.reg",
                "accounts.json",
                "launch.log",
                "cache/file",
            ] {
                assert_eq!(fs::read(root.join(name)).unwrap(), b"keep");
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn interrupted_replacement_restores_old_files_before_launch() {
        let root = root();
        let client = ClientId::OfficialRs3;
        put(&root, "versions/old/client", b"old");
        put(&root, "current.json", b"old-record");
        put(&root, "data/settings", b"keep");
        let folder = transaction(&root);
        fs::create_dir_all(folder.join("files")).unwrap();
        let journal = Journal {
            client,
            entries: vec![
                Entry {
                    name: "versions".into(),
                    existed: true,
                },
                Entry {
                    name: "runtime".into(),
                    existed: false,
                },
                Entry {
                    name: "current.json".into(),
                    existed: true,
                },
            ],
        };
        downloads::activate_json(&folder, &journal).unwrap();
        fs::rename(root.join("versions"), folder.join("files/versions")).unwrap();
        put(&root, "versions/new/client", b"new");
        put(&root, "runtime/lib", b"new");
        recover(&root, client).unwrap();
        assert_eq!(fs::read(root.join("versions/old/client")).unwrap(), b"old");
        assert!(!root.join("versions/new").exists());
        assert!(!root.join("runtime").exists());
        assert_eq!(fs::read(root.join("current.json")).unwrap(), b"old-record");
        assert_eq!(fs::read(root.join("data/settings")).unwrap(), b"keep");
        recover(&root, client).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn committed_replacement_is_kept_and_missing_journal_never_deletes_backups() {
        let root = root();
        let client = ClientId::OfficialRs3;
        let folder = transaction(&root);
        put(&root, "versions/new/file", b"new");
        put(&root, "current.json", b"new-record");
        put(&folder, "files/versions/old/file", b"old");
        let journal = Journal {
            client,
            entries: paths(client)
                .iter()
                .map(|name| Entry {
                    name: (*name).into(),
                    existed: true,
                })
                .collect(),
        };
        downloads::activate_json(&folder, &journal).unwrap();
        put(&folder, "committed", b"");
        recover(&root, client).unwrap();
        assert_eq!(fs::read(root.join("versions/new/file")).unwrap(), b"new");
        assert!(!folder.exists());
        put(&folder, "files/current.json", b"old-record");
        assert!(recover(&root, client).is_err());
        assert_eq!(
            fs::read(folder.join("files/current.json")).unwrap(),
            b"old-record"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn repaired_copy_is_activated_without_changing_settings() {
        let root = root();
        let fresh = root.join(".repair");
        fs::create_dir(&fresh).unwrap();
        put(&root, "versions/old/file", b"old");
        put(&root, "current.json", b"old-record");
        put(&root, "wine-prefix/user.reg", b"keep");
        put(&fresh, "versions/new/file", b"new");
        put(&fresh, "current.json", b"new-record");
        activate(&root, Some(&fresh), ClientId::OfficialOsrs).unwrap();
        assert_eq!(fs::read(root.join("current.json")).unwrap(), b"new-record");
        assert_eq!(fs::read(root.join("versions/new/file")).unwrap(), b"new");
        assert!(!root.join("versions/old").exists());
        assert_eq!(
            fs::read(root.join("wine-prefix/user.reg")).unwrap(),
            b"keep"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recovery_rejects_paths_outside_the_managed_file_list() {
        let root = root();
        let folder = transaction(&root);
        fs::create_dir(&folder).unwrap();
        let record = Journal {
            client: ClientId::OfficialOsrs,
            entries: vec![Entry {
                name: "../accounts.json".into(),
                existed: true,
            }],
        };
        downloads::activate_json(&folder, &record).unwrap();
        assert!(recover(&root, ClientId::OfficialOsrs).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn a_game_outliving_the_launcher_is_detected_without_reading_credentials() {
        let root = root();
        let mut child = std::process::Command::new("python3")
            .args(["-c", "import time; time.sleep(30)"])
            .arg(root.join("versions/game"))
            .spawn()
            .unwrap();
        let blocked = ensure_closed(&root).is_err();
        let _ = child.kill();
        let _ = child.wait();
        assert!(blocked);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn uninstall_does_not_follow_links_into_user_data() {
        let root = root();
        let external = root.join("user-settings");
        fs::create_dir(&external).unwrap();
        put(&external, "keep", b"keep");
        std::os::unix::fs::symlink(&external, root.join("versions")).unwrap();
        activate(&root, None, ClientId::OfficialOsrs).unwrap();
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
    fn live_client_recovery(client_id: ClientId) {
        let root = root();
        let client = root.join(client_id.directory());
        let progress = Mutex::new(Progress::default());
        put(&client, "current.json", b"damaged install record");
        put(&client, "data/settings", b"keep");
        put(&client, "wine-prefix/user.reg", b"keep");
        put(&root, "accounts.json", b"keep");
        tauri::async_runtime::block_on(run(&root, client_id, Operation::Repair, &progress))
            .unwrap();
        assert!(client_id.installed_version(&root).is_some());
        tauri::async_runtime::block_on(run(&root, client_id, Operation::Uninstall, &progress))
            .unwrap();
        assert!(!has_files(&client, client_id));
        tauri::async_runtime::block_on(run(&root, client_id, Operation::Repair, &progress))
            .unwrap();
        assert!(client_id.installed_version(&root).is_some());
        for path in [
            client.join("data/settings"),
            client.join("wine-prefix/user.reg"),
            root.join("accounts.json"),
        ] {
            assert_eq!(fs::read(path).unwrap(), b"keep");
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "downloads RuneLite and Java twice into isolated recovery folders"]
    fn live_recovery_runelite_repair_uninstall_reinstall() {
        live_client_recovery(ClientId::Runelite);
    }
    #[test]
    #[ignore = "downloads official OSRS twice into isolated recovery folders"]
    fn live_recovery_osrs_repair_uninstall_reinstall() {
        live_client_recovery(ClientId::OfficialOsrs);
    }
    #[test]
    #[ignore = "downloads RS3 twice to verify repair/uninstall/reinstall without touching user installations"]
    fn live_rs3_repair_uninstall_and_reinstall_preserve_settings() {
        let root = root();
        let client = root.join(ClientId::OfficialRs3.directory());
        let progress = Mutex::new(Progress::default());
        tauri::async_runtime::block_on(rs3::ensure(&client, &progress)).unwrap();
        put(&client, "data/settings", b"keep");
        put(&root, "accounts.json", b"keep");
        let result = tauri::async_runtime::block_on(run(
            &root,
            ClientId::OfficialRs3,
            Operation::Repair,
            &progress,
        ));
        result.unwrap();
        assert!(rs3::installed(&client).is_some());
        tauri::async_runtime::block_on(run(
            &root,
            ClientId::OfficialRs3,
            Operation::Uninstall,
            &progress,
        ))
        .unwrap();
        assert!(rs3::installed(&client).is_none());
        tauri::async_runtime::block_on(rs3::ensure(&client, &progress)).unwrap();
        assert_eq!(fs::read(client.join("data/settings")).unwrap(), b"keep");
        assert_eq!(fs::read(root.join("accounts.json")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
}
