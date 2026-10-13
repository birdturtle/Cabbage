use std::process::Command;

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Debug)]
enum Offload {
    Nvidia,
    Mesa,
    Default,
}

#[cfg(any(target_os = "linux", test))]
fn select(vendors: &[String], nvidia_driver: bool) -> Offload {
    if vendors.len() < 2 {
        return Offload::Default;
    }
    if nvidia_driver && vendors.iter().any(|vendor| vendor == "0x10de") {
        Offload::Nvidia
    } else {
        Offload::Mesa
    }
}

#[cfg(any(target_os = "linux", test))]
fn apply(command: &mut Command, enabled: bool, offload: Offload) {
    if !enabled {
        return;
    }
    match offload {
        Offload::Nvidia => {
            command
                .env("__NV_PRIME_RENDER_OFFLOAD", "1")
                .env("__GLX_VENDOR_LIBRARY_NAME", "nvidia")
                .env("__VK_LAYER_NV_optimus", "NVIDIA_only");
        }
        Offload::Mesa => {
            command.env("DRI_PRIME", "1");
        }
        Offload::Default => {}
    }
}

pub fn configure(command: &mut Command, enabled: bool) {
    #[cfg(target_os = "linux")]
    {
        let vendors: Vec<_> = std::fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                let index = name.strip_prefix("card")?;
                if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                std::fs::read_to_string(entry.path().join("device/vendor"))
                    .ok()
                    .map(|s| s.trim().to_ascii_lowercase())
            })
            .collect();
        let nvidia_driver = std::path::Path::new("/sys/module/nvidia").is_dir();
        apply(command, enabled, select(&vendors, nvidia_driver));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (command, enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hybrid_selection_and_disabled_preference_preserve_system_defaults() {
        assert_eq!(
            select(&["0x1002".into(), "0x10de".into()], true),
            Offload::Nvidia
        );
        assert_eq!(
            select(&["0x8086".into(), "0x1002".into()], false),
            Offload::Mesa
        );
        assert_eq!(select(&["0x10de".into()], true), Offload::Default);
        assert_eq!(select(&[], false), Offload::Default);
        assert_eq!(
            select(&["0x1002".into(), "0x10de".into()], false),
            Offload::Mesa
        );
        let mut command = Command::new("game");
        apply(&mut command, false, Offload::Nvidia);
        assert_eq!(command.get_envs().count(), 0);
        apply(&mut command, true, Offload::Nvidia);
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "__NV_PRIME_RENDER_OFFLOAD"
                && value == Some(std::ffi::OsStr::new("1"))));
        assert_eq!(command.get_args().count(), 0);
        let mut mesa = Command::new("game");
        apply(&mut mesa, true, Offload::Mesa);
        assert!(mesa
            .get_envs()
            .any(|(key, value)| key == "DRI_PRIME" && value == Some(std::ffi::OsStr::new("1"))));
    }
}
