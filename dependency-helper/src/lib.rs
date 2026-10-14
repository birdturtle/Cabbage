use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Client {
    Runelite,
    OfficialOsrs,
    OfficialRs3,
}
impl Client {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "runelite" => Ok(Self::Runelite),
            "officialOsrs" => Ok(Self::OfficialOsrs),
            "officialRs3" => Ok(Self::OfficialRs3),
            _ => Err("Unknown game client.".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Runelite => "RuneLite",
            Self::OfficialOsrs => "Official Old School RuneScape",
            Self::OfficialRs3 => "RuneScape",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Requirement {
    pub name: String,
    pub package: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    Pacman,
    Apt { t64: bool },
    Unsupported,
}
impl Backend {
    pub fn executable(self) -> &'static str {
        match self {
            Self::Pacman => "/usr/bin/pacman",
            Self::Apt { .. } => "/usr/bin/apt-get",
            Self::Unsupported => "",
        }
    }
    pub fn notice(self) -> &'static str {
        match self {
            Self::Pacman => "This also updates installed system packages.\nPacman will show the full transaction before you approve it.",
            _ => "Package lists will be refreshed before installation.\nAPT will show the full transaction before you approve it.",
        }
    }
    fn package(self, arch: &str) -> &str {
        if let Self::Apt { t64 } = self {
            match arch {
                "xorg-xwayland" => "xwayland",
                "sdl2-compat" => "libsdl2-2.0-0",
                "libx11" => "libx11-6",
                "libxext" => "libxext6",
                "libxi" => "libxi6",
                "libxrender" => "libxrender1",
                "libxtst" => "libxtst6",
                "freetype2" => "libfreetype6",
                "fontconfig" => "libfontconfig1",
                "alsa-lib" if t64 => "libasound2t64",
                "alsa-lib" => "libasound2",
                _ => arch,
            }
        } else {
            arch
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub client: Client,
    pub system: String,
    pub backend: Backend,
    pub supported: bool,
    pub missing: Vec<Requirement>,
}
impl Plan {
    pub fn packages(&self) -> Vec<String> {
        self.missing
            .iter()
            .map(|r| r.package.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn arguments(&self) -> Result<Vec<String>, String> {
        if !self.supported || self.missing.is_empty() {
            return Err("There is no supported installation to run.".into());
        }
        let packages = self.packages();
        let allowed: &[&str] = match self.backend {
            Backend::Pacman => &[
                "wine",
                "sdl2-compat",
                "libglvnd",
                "xorg-xwayland",
                "gnome-keyring",
                "libx11",
                "libxext",
                "libxi",
                "libxrender",
                "libxtst",
                "freetype2",
                "fontconfig",
                "alsa-lib",
            ],
            Backend::Apt { t64 } => &[
                "wine",
                "libsdl2-2.0-0",
                "libegl1",
                "libopengl0",
                "libgl1",
                "xwayland",
                "gnome-keyring",
                "libx11-6",
                "libxext6",
                "libxi6",
                "libxrender1",
                "libxtst6",
                "libfreetype6",
                "libfontconfig1",
                if t64 { "libasound2t64" } else { "libasound2" },
            ],
            Backend::Unsupported => return Err("Unsupported package manager.".into()),
        };
        if packages.iter().any(|p| !allowed.contains(&p.as_str())) {
            return Err("Unsupported dependency package.".into());
        }
        let mut args: Vec<String> = match self.backend {
            Backend::Pacman => ["--", "/usr/bin/pacman", "-Syu", "--"]
                .iter()
                .map(|s| (*s).into())
                .collect(),
            Backend::Apt { .. } => [
                "--",
                "/usr/bin/apt-get",
                "install",
                "--no-remove",
                "--reinstall",
                "-o",
                "APT::Get::Assume-Yes=false",
                "-o",
                "APT::Get::Assume-No=false",
                "--",
            ]
            .iter()
            .map(|s| (*s).into())
            .collect(),
            Backend::Unsupported => unreachable!(),
        };
        args.extend(packages);
        Ok(args)
    }
}
pub fn os_release(text: &str) -> (String, Backend) {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            line.strip_prefix(&format!("{key}="))
                .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
        })
    };
    let id = value("ID").unwrap_or_default();
    (
        value("PRETTY_NAME").unwrap_or_else(|| id.clone()),
        match (id.as_str(), value("VERSION_ID").as_deref()) {
            ("arch" | "cachyos", _) => Backend::Pacman,
            ("debian", Some("12")) | ("ubuntu", Some("22.04")) => Backend::Apt { t64: false },
            ("debian", Some("13")) | ("ubuntu", Some("24.04")) => Backend::Apt { t64: true },
            _ => Backend::Unsupported,
        },
    )
}
/// Runs the approved transaction; failures stop subsequent commands.
/// The runner receives sudo arguments, never shell text.
pub fn transaction(
    plan: &Plan,
    mut run: impl FnMut(&[String]) -> Result<bool, String>,
) -> Result<(), String> {
    let install = plan.arguments()?;
    if matches!(plan.backend, Backend::Apt { .. }) {
        let refresh = ["--", "/usr/bin/apt-get", "update", "--error-on=any"].map(String::from);
        if !run(&refresh)? {
            return Err(
                "Package lists could not be refreshed. Check the terminal output and retry.".into(),
            );
        }
    }
    if !run(&install)? {
        return Err(
            "Package installation did not finish. Check the terminal output and retry when ready."
                .into(),
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn executable(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            fs::metadata(dir.join(name))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}
#[cfg(target_os = "linux")]
fn library(name: &str) -> bool {
    use std::io::Read;
    [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
    ]
    .iter()
    .any(|dir| {
        let mut header = [0u8; 20];
        fs::File::open(Path::new(dir).join(name)).is_ok_and(|mut file| {
            file.read_exact(&mut header).is_ok()
                && header[..4] == *b"\x7fELF"
                && header[4] == 2
                && header[5] == 1
                && u16::from_le_bytes([header[18], header[19]]) == 62
        })
    })
}
#[cfg(target_os = "linux")]
fn keyring_provider() -> bool {
    // Existing providers need unlock/configuration help, not a replacement keyring.
    [
        "gnome-keyring-daemon",
        "kwalletd6",
        "kwalletd5",
        "keepassxc",
    ]
    .iter()
    .any(|name| executable(name))
        || fs::read_dir("/usr/share/dbus-1/services").is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                fs::read_to_string(entry.path()).is_ok_and(|text| {
                    text.lines()
                        .any(|line| line.trim() == "Name=org.freedesktop.secrets")
                })
            })
        })
}
#[cfg(target_os = "linux")]
pub fn probe(client: Client) -> Plan {
    let (system, backend) = os_release(&fs::read_to_string("/etc/os-release").unwrap_or_default());
    let mut missing = Vec::new();
    let mut add = |name: &str, package: &str| {
        missing.push(Requirement {
            name: name.into(),
            package: backend.package(package).into(),
        })
    };
    if !keyring_provider() {
        add("Desktop keyring", "gnome-keyring");
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && !executable("Xwayland") {
        add("XWayland", "xorg-xwayland");
    }
    match client {
        Client::OfficialOsrs => {
            if !executable("wine") {
                add("Wine", "wine");
            }
        }
        Client::OfficialRs3 => {
            for (file, name, package) in [
                ("libSDL2-2.0.so.0", "SDL2", "sdl2-compat"),
                ("libEGL.so.1", "EGL", "libglvnd"),
                ("libOpenGL.so.0", "OpenGL", "libglvnd"),
            ] {
                if !library(file) {
                    let package = if matches!(backend, Backend::Apt { .. }) && package == "libglvnd"
                    {
                        match file {
                            "libEGL.so.1" => "libegl1",
                            "libOpenGL.so.0" => "libopengl0",
                            _ => "libgl1",
                        }
                    } else {
                        package
                    };
                    add(name, package);
                }
            }
        }
        Client::Runelite => {
            for (file, name, package) in [
                ("libX11.so.6", "X11", "libx11"),
                ("libXext.so.6", "X11 extensions", "libxext"),
                ("libXi.so.6", "X11 input", "libxi"),
                ("libXrender.so.1", "X11 rendering", "libxrender"),
                ("libXtst.so.6", "X11 testing library", "libxtst"),
                ("libfreetype.so.6", "FreeType", "freetype2"),
                ("libfontconfig.so.1", "Fontconfig", "fontconfig"),
                ("libasound.so.2", "ALSA", "alsa-lib"),
                ("libGL.so.1", "OpenGL", "libglvnd"),
            ] {
                if !library(file) {
                    let package = if matches!(backend, Backend::Apt { .. }) && package == "libglvnd"
                    {
                        match file {
                            "libEGL.so.1" => "libegl1",
                            "libOpenGL.so.0" => "libopengl0",
                            _ => "libgl1",
                        }
                    } else {
                        package
                    };
                    add(name, package);
                }
            }
        }
    }
    Plan {
        client,
        system,
        backend,
        supported: backend != Backend::Unsupported
            && std::env::consts::ARCH == "x86_64"
            && Path::new(backend.executable()).is_file(),
        missing,
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Waiting,
    Installing,
    Finished,
    Cancelled,
    Failed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub token: String,
    pub pid: u32,
    pub phase: Phase,
    pub message: String,
}
pub fn write_report(path: &Path, report: &Report) -> Result<(), String> {
    use std::io::Write;
    if !path.is_absolute()
        || report.token.len() != 64
        || !report.token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid setup session.".into());
    }
    let stage = path.with_extension("next");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&stage)
            .map_err(|_| "Cannot save setup status.")?;
        file.write_all(&serde_json::to_vec(report).map_err(|_| "Cannot encode setup status.")?)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save setup status.")?;
        drop(file);
        fs::rename(&stage, path).map_err(|_| "Cannot publish setup status.".to_string())
    })();
    let _ = fs::remove_file(stage);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arch_plans_include_system_update_and_never_bypass_pacman_consent() {
        let mut plan = Plan {
            client: Client::OfficialRs3,
            system: "CachyOS".into(),
            backend: Backend::Pacman,
            supported: true,
            missing: vec![
                Requirement {
                    name: "EGL".into(),
                    package: "libglvnd".into(),
                },
                Requirement {
                    name: "OpenGL".into(),
                    package: "libglvnd".into(),
                },
            ],
        };
        assert_eq!(
            plan.arguments().unwrap(),
            ["--", "/usr/bin/pacman", "-Syu", "--", "libglvnd"]
        );
        plan.missing[0].package = "--noconfirm".into();
        assert!(plan.arguments().is_err());
        plan.supported = false;
        assert!(plan.arguments().is_err());
    }
    #[test]
    fn apt_plans_match_release_packages_and_keep_interactive_consent() {
        for (release, t64) in [
            ("ID=debian\nVERSION_ID=12", false),
            ("ID=debian\nVERSION_ID=13", true),
            ("ID=ubuntu\nVERSION_ID=22.04", false),
            ("ID=ubuntu\nVERSION_ID=24.04", true),
        ] {
            let backend = os_release(release).1;
            assert_eq!(backend, Backend::Apt { t64 });
            let mut plan = Plan {
                client: Client::Runelite,
                system: "Test".into(),
                backend,
                supported: true,
                missing: vec![Requirement {
                    name: "ALSA".into(),
                    package: backend.package("alsa-lib").into(),
                }],
            };
            let args = plan.arguments().unwrap();
            assert_eq!(
                args.last().unwrap(),
                if t64 { "libasound2t64" } else { "libasound2" }
            );
            assert!(args.contains(&"--no-remove".into()));
            assert!(!args
                .iter()
                .any(|s| matches!(s.as_str(), "-y" | "upgrade" | "dist-upgrade")));
            for invalid in ["wine-", "--assume-yes", "libglvnd", "libasound2*"] {
                plan.missing[0].package = invalid.into();
                assert!(plan.arguments().is_err());
            }
        }
        assert_eq!(
            os_release("ID=linuxmint\nID_LIKE=ubuntu\nVERSION_ID=22").1,
            Backend::Unsupported
        );
        assert_eq!(
            os_release("ID=ubuntu\nVERSION_ID=20.04").1,
            Backend::Unsupported
        );
    }
    #[test]
    fn apt_refresh_failure_prevents_install_and_cancel_is_a_failure() {
        let plan = Plan {
            client: Client::OfficialOsrs,
            system: "Debian".into(),
            backend: Backend::Apt { t64: true },
            supported: true,
            missing: vec![Requirement {
                name: "Wine".into(),
                package: "wine".into(),
            }],
        };
        let mut calls = Vec::new();
        assert!(transaction(&plan, |args| {
            calls.push(args.to_vec());
            Ok(false)
        })
        .is_err());
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ["--", "/usr/bin/apt-get", "update", "--error-on=any"]
        );
        calls.clear();
        assert!(transaction(&plan, |args| {
            calls.push(args.to_vec());
            Ok(calls.len() == 1)
        })
        .is_err());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1][2], "install");
        assert!(transaction(&plan, |_| Ok(true)).is_ok());
    }
    #[test]
    fn session_reports_replace_atomically_without_exposing_other_files() {
        let root = std::env::temp_dir().join(format!(
            "cabbage-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("status.json");
        for phase in [Phase::Waiting, Phase::Installing, Phase::Finished] {
            let report = Report {
                token: "a".repeat(64),
                pid: std::process::id(),
                phase,
                message: "status".into(),
            };
            write_report(&path, &report).unwrap();
            let read: Report = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(read.phase, phase);
            assert_eq!(read.token, report.token);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn distro_support_is_explicit() {
        assert_eq!(
            os_release("ID=cachyos\nPRETTY_NAME=\"CachyOS\"").1,
            Backend::Pacman
        );
        assert_eq!(os_release("ID=arch").1, Backend::Pacman);
        assert_eq!(
            os_release("ID=fedora\nID_LIKE=arch").1,
            Backend::Unsupported
        );
        assert_eq!(
            os_release("ID=manjaro\nID_LIKE=arch").1,
            Backend::Unsupported
        );
    }
}
