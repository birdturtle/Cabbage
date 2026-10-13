use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

static INSTALL_LOCK: Mutex<()> = Mutex::new(());
#[cfg(not(windows))]
const ICON: &[u8] = include_bytes!("../icons/icon.png");
#[cfg(windows)]
const ICON: &[u8] = include_bytes!("../icons/icon.ico");
#[cfg(windows)]
#[path = "installation_windows.rs"]
mod windows;
const ID: &str = "com.birdturtle.cabbage";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub can_install: bool,
    pub offer_install: bool,
}
#[derive(Serialize, Deserialize)]
struct Marker {
    mode: String,
    desktop: String,
}

pub struct Layout {
    root: PathBuf,
    desktop: PathBuf,
    icon: PathBuf,
}
impl Layout {
    pub fn new(data: &Path) -> Self {
        Self {
            root: data.join("cabbage/launcher"),
            desktop: if cfg!(windows) {
                data.join("Start Menu/Programs/Cabbage.lnk")
            } else {
                data.join("applications").join(format!("{ID}.desktop"))
            },
            icon: if cfg!(windows) {
                data.join("cabbage/launcher/Cabbage.ico")
            } else {
                data.join("icons/hicolor/256x256/apps")
                    .join(format!("{ID}.png"))
            },
        }
    }
    #[cfg(windows)]
    pub fn windows(data: &Path, roaming: &Path) -> Self {
        let mut layout = Self::new(data);
        layout.desktop = roaming.join("Microsoft/Windows/Start Menu/Programs/Cabbage.lnk");
        layout.icon = layout.root.join("Cabbage.ico");
        layout
    }
    fn binary(&self) -> PathBuf {
        self.root.join(if cfg!(windows) {
            "Cabbage.exe"
        } else {
            "Cabbage.AppImage"
        })
    }
    fn marker(&self) -> PathBuf {
        self.root.join("installation.json")
    }
    fn installed(&self) -> bool {
        fs::read(self.marker())
            .ok()
            .and_then(|b| serde_json::from_slice::<Marker>(&b).ok())
            .is_some_and(|m| m.mode == install_mode())
            && regular(&self.binary())
    }
    fn desktop_text(&self) -> Result<String, String> {
        let executable = self.binary();
        let executable = executable
            .to_str()
            .ok_or("Installation path must be UTF-8.")?;
        let executable = executable
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%");
        let icon = self
            .icon
            .to_str()
            .ok_or("Installation path must be UTF-8.")?
            .replace('\\', "\\\\");
        if executable.contains(['\n', '\r']) || icon.contains(['\n', '\r']) {
            return Err("Invalid installation path.".into());
        }
        Ok(format!("[Desktop Entry]\nType=Application\nName=Cabbage\nExec=\"{executable}\"\nIcon={icon}\nStartupWMClass={ID}\nTerminal=false\nCategories=Game;Utility;\n"))
    }
}
fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}
fn safe_directory(path: &Path) -> Result<(), String> {
    for parent in path.ancestors() {
        if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Installation directory must not be a symbolic link.".into());
        }
    }
    fs::create_dir_all(path).map_err(|_| "Cannot create installation directory.")?;
    Ok(())
}
fn atomic_file(path: &Path, bytes: &[u8], executable: bool) -> Result<(), String> {
    safe_directory(path.parent().ok_or("Invalid installation path.")?)?;
    let staged = path.with_extension(format!("stage-{:x}", rand::random::<u64>()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(if executable { 0o700 } else { 0o600 });
        }
        let mut file = options
            .open(&staged)
            .map_err(|_| "Cannot write installation files.")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save installation files.")?;
        drop(file);
        fs::rename(&staged, path).map_err(|_| "Cannot activate installation files.")
    })();
    let _ = fs::remove_file(staged);
    Ok(result?)
}
fn appimage_source() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("APPIMAGE")?);
    if !path.is_absolute() {
        return None;
    }
    let path = fs::canonicalize(path).ok()?;
    valid_image(&path).then_some(path)
}
fn valid_image(path: &Path) -> bool {
    let mut header = [0; 11];
    regular(path)
        && fs::File::open(path).is_ok_and(|mut f| f.read_exact(&mut header).is_ok())
        && &header[..4] == b"\x7fELF"
        && &header[8..11] == b"AI\x02"
}
fn install_mode() -> &'static str {
    if cfg!(windows) {
        "windowsExe"
    } else {
        "appimage"
    }
}
pub fn source() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if cfg!(debug_assertions) {
            return None;
        }
        return std::env::current_exe()
            .ok()
            .filter(|path| valid_source(path));
    }
    #[cfg(not(windows))]
    {
        appimage_source()
    }
}
fn valid_source(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut header = [0; 2];
        regular(path)
            && fs::File::open(path).is_ok_and(|mut f| f.read_exact(&mut header).is_ok())
            && &header == b"MZ"
    }
    #[cfg(not(windows))]
    {
        valid_image(path)
    }
}
pub fn status(layout: &Layout) -> Status {
    let installed = layout.installed();
    let can_install = source().is_some();
    Status {
        installed,
        can_install,
        offer_install: can_install && !installed && !layout.root.join("portable.json").exists(),
    }
}
pub fn keep_portable(layout: &Layout) -> Result<(), String> {
    let _lock = INSTALL_LOCK.lock().unwrap();
    atomic_file(&layout.root.join("portable.json"), b"true", false)
}
pub fn install(layout: &Layout, source: &Path) -> Result<(), String> {
    let _lock = INSTALL_LOCK.lock().unwrap();
    if !valid_source(source) {
        return Err("Install Cabbage from its launcher download.".into());
    }
    safe_directory(&layout.root)?;
    #[cfg(not(windows))]
    let desktop = layout.desktop_text()?;
    if source != layout.binary() {
        if fs::metadata(source)
            .map_err(|_| "Cannot read launcher download.")?
            .len()
            > 512 * 1024 * 1024
        {
            return Err("Launcher download is too large.".into());
        }
        let bytes = fs::read(source).map_err(|_| "Cannot read launcher download.")?;
        if !fs::read(layout.binary()).is_ok_and(|installed| installed == bytes) {
            atomic_file(&layout.binary(), &bytes, true)?;
        }
    }
    atomic_file(&layout.icon, ICON, false)?;
    #[cfg(not(windows))]
    atomic_file(&layout.desktop, desktop.as_bytes(), false)?;
    #[cfg(windows)]
    let desktop = windows::integrate(layout)?;
    let marker = Marker {
        mode: install_mode().into(),
        desktop,
    };
    atomic_file(
        &layout.marker(),
        &serde_json::to_vec(&marker).map_err(|_| "Cannot record installation.")?,
        false,
    )?;
    refresh_desktop(layout);
    Ok(())
}
pub fn uninstall(layout: &Layout) -> Result<bool, String> {
    let _lock = INSTALL_LOCK.lock().unwrap();
    if !layout.installed() {
        return Err("No Cabbage installation was found.".into());
    }
    safe_directory(&layout.root)?;
    #[cfg(windows)]
    {
        windows::uninstall(layout)
    }
    #[cfg(not(windows))]
    {
        let marker: Marker = serde_json::from_slice(
            &fs::read(layout.marker()).map_err(|_| "Cannot read installation record.")?,
        )
        .map_err(|_| "Invalid installation record.")?;
        if fs::read(&layout.desktop).is_ok_and(|b| b == marker.desktop.as_bytes()) {
            fs::remove_file(&layout.desktop)
                .map_err(|_| "Cannot remove application-menu entry.")?;
        }
        if fs::read(&layout.icon).is_ok_and(|b| b == ICON) {
            fs::remove_file(&layout.icon).map_err(|_| "Cannot remove application icon.")?;
        }
        fs::remove_file(layout.binary()).map_err(|_| "Cannot remove installed launcher.")?;
        fs::remove_file(layout.marker()).map_err(|_| "Cannot remove installation record.")?;
        atomic_file(&layout.root.join("portable.json"), b"true", false)?;
        refresh_desktop(layout);
        Ok(false)
    }
}
fn refresh_desktop(_layout: &Layout) {
    #[cfg(all(target_os = "linux", not(test)))]
    for (program, args) in [
        (
            "update-desktop-database",
            vec![_layout
                .desktop
                .parent()
                .unwrap()
                .to_string_lossy()
                .into_owned()],
        ),
        ("kbuildsycoca6", vec!["--noincremental".into()]),
    ] {
        let _ = std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_and_uninstall_keep_accounts_and_games() {
        let root = std::env::temp_dir().join(format!("cabbage install {}", rand::random::<u64>()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("download.AppImage");
        fs::write(
            &source,
            if cfg!(windows) {
                &b"MZfixture"[..]
            } else {
                &b"\x7fELF\0\0\0\0AI\x02fixture"[..]
            },
        )
        .unwrap();
        let vault = root.join("com.birdturtle.cabbage");
        fs::create_dir_all(&vault).unwrap();
        fs::write(vault.join("accounts"), b"preserve").unwrap();
        let layout = Layout::new(&root);
        install(&layout, &source).unwrap();
        assert!(layout.installed());
        #[cfg(not(windows))]
        assert!(fs::read_to_string(&layout.desktop)
            .unwrap()
            .contains("Cabbage.AppImage\""));
        install(&layout, &source).unwrap();
        uninstall(&layout).unwrap();
        assert!(!layout.installed());
        assert!(!layout.desktop.exists());
        assert_eq!(fs::read(vault.join("accounts")).unwrap(), b"preserve");
        assert!(source.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unowned_files_and_invalid_downloads_are_not_removed() {
        let root = std::env::temp_dir().join(format!("cabbage install {}", rand::random::<u64>()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("download");
        fs::write(&source, b"wrong").unwrap();
        let layout = Layout::new(&root);
        assert!(install(&layout, &source).is_err());
        assert!(uninstall(&layout).is_err());
        assert!(layout
            .desktop_text()
            .unwrap()
            .contains("StartupWMClass=com.birdturtle.cabbage"));
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(windows)]
pub fn windows_cli_uninstall() -> Result<(), String> {
    let local = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("Cannot locate installation directory.")?,
    );
    let roaming =
        PathBuf::from(std::env::var_os("APPDATA").ok_or("Cannot locate Start Menu directory.")?);
    uninstall(&Layout::windows(&local, &roaming))?;
    Ok(())
}
