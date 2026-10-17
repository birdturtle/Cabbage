//! One-time local RuneLite settings copy and a stable developer profile across plugin updates.
use std::{fs, path::Path};

pub fn initialize(home: &Path, normal_home: &Path) -> Result<(), String> {
    if home.exists() {
        if !home.is_dir() || !home.join(".runelite").is_dir() {
            return Err("The developer profile folder is incomplete or invalid. Existing files have been left unchanged.".into());
        }
        return Ok(());
    }
    let staging = home.with_extension(format!("{:x}.staging", rand::random::<u64>()));
    let result = (|| {
        let destination = staging.join(".runelite");
        fs::create_dir_all(&destination).map_err(|_| "Cannot create developer profile.")?;
        let source = normal_home.join(".runelite");
        let mut budget = Budget { files: 0, bytes: 0 };
        // Configuration profiles include the installed Hub plugin list. RuneLite downloads those plugins itself.
        // Never copy Jagex credentials, RuneLite account sessions, sideloaded code, caches or logs.
        for name in [
            "profiles2",
            "settings.properties",
            "plugin-data",
            "resource-packs-repository",
        ] {
            let from = source.join(name);
            if name != "settings.properties"
                && fs::symlink_metadata(&from).is_ok_and(|m| m.is_file())
            {
                return Err("An existing RuneLite profile directory is a file. The original setup has been left unchanged.".into());
            }
            copy(&from, &destination.join(name), &mut budget, 0)?;
        }
        fs::rename(&staging, home).map_err(|_| "Cannot activate the copied RuneLite profile.")?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}
struct Budget {
    files: usize,
    bytes: u64,
}
fn copy(
    source: &Path,
    destination: &Path,
    budget: &mut Budget,
    depth: usize,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Cannot read existing RuneLite profile files.".into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if source.extension().is_some_and(|e| e == "lck") {
        return Ok(());
    }
    budget.files += 1;
    if budget.files > 20_000 || depth > 32 {
        return Err("RuneLite profile has too many files to copy automatically.".into());
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|_| "Cannot create copied profile directory.")?;
        for entry in
            fs::read_dir(source).map_err(|_| "Cannot read existing RuneLite profile directory.")?
        {
            let entry = entry.map_err(|_| "Cannot read existing RuneLite profile entry.")?;
            copy(
                &entry.path(),
                &destination.join(entry.file_name()),
                budget,
                depth + 1,
            )?;
        }
    } else if metadata.is_file() {
        budget.bytes = budget
            .bytes
            .checked_add(metadata.len())
            .ok_or("RuneLite profile is too large.")?;
        if budget.bytes > 512 * 1024 * 1024 {
            return Err("RuneLite profile exceeds the 512 MiB automatic-copy limit.".into());
        }
        fs::copy(source, destination).map_err(|_| "Cannot copy existing RuneLite profile file.")?;
    }
    Ok(())
}

pub fn activate_plugins(root: &Path, home: &Path, jars: &[Vec<u8>]) -> Result<(), String> {
    let profile = home.join(".runelite");
    let target = profile.join("sideloaded-plugins");
    if fs::symlink_metadata(&target).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
        return Err("The developer plugin folder is not a regular directory. Existing files have been left unchanged.".into());
    }
    let matches = target.is_dir()
        && fs::read_dir(&target)
            .map_err(|_| "Cannot inspect developer plugins.")?
            .count()
            == jars.len()
        && jars.iter().enumerate().all(|(i, bytes)| {
            let path = target.join(format!("plugin-{i}.jar"));
            fs::symlink_metadata(&path).is_ok_and(|m| {
                m.is_file() && !m.file_type().is_symlink() && m.len() == bytes.len() as u64
            }) && fs::read(path).is_ok_and(|old| old == *bytes)
        });
    if matches {
        return Ok(());
    }
    crate::maintenance::ensure_closed(root).map_err(|_| {
        "Close RuneLite before changing developer plugins. Your copied profile will be kept."
    })?;
    let staging = profile.join(format!("plugins-{:x}.staging", rand::random::<u64>()));
    let backup = profile.join(format!("plugins-{:x}.previous", rand::random::<u64>()));
    fs::create_dir(&staging).map_err(|_| "Cannot stage developer plugins.")?;
    let result = (|| {
        for (i, bytes) in jars.iter().enumerate() {
            fs::write(staging.join(format!("plugin-{i}.jar")), bytes)
                .map_err(|_| "Cannot stage developer plugin.")?;
        }
        if target.exists() {
            fs::rename(&target, &backup).map_err(|_| "Cannot replace developer plugins.")?;
        }
        if fs::rename(&staging, &target).is_err() {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err("Cannot activate developer plugins.".into());
        }
        let _ = fs::remove_dir_all(&backup);
        Ok(())
    })();
    let _ = fs::remove_dir_all(staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copies_profile_plugin_choices_and_data_once_but_excludes_sessions_and_code() {
        let root =
            std::env::temp_dir().join(format!("cabbage profile {:x}", rand::random::<u64>()));
        let normal = root.join("normal");
        let source = normal.join(".runelite");
        fs::create_dir_all(source.join("profiles2")).unwrap();
        fs::create_dir_all(source.join("plugin-data/example")).unwrap();
        fs::write(source.join("profiles2/profiles.json"), b"profile selection").unwrap();
        fs::write(
            source.join("profiles2/default-1.properties"),
            b"runelite.externalPlugins=example",
        )
        .unwrap();
        fs::write(source.join("plugin-data/example/settings.json"), b"data").unwrap();
        for name in [
            "credentials.properties",
            "session",
            "profiles2/profiles.lck",
        ] {
            fs::write(source.join(name), b"excluded").unwrap();
        }
        fs::create_dir_all(source.join("sideloaded-plugins")).unwrap();
        fs::write(source.join("sideloaded-plugins/unselected.jar"), b"code").unwrap();
        let home = root.join("developer");
        initialize(&home, &normal).unwrap();
        assert_eq!(
            fs::read(home.join(".runelite/profiles2/default-1.properties")).unwrap(),
            b"runelite.externalPlugins=example"
        );
        assert!(home
            .join(".runelite/plugin-data/example/settings.json")
            .exists());
        for name in [
            "credentials.properties",
            "session",
            "profiles2/profiles.lck",
            "sideloaded-plugins",
        ] {
            assert!(!home.join(".runelite").join(name).exists());
        }
        fs::write(
            home.join(".runelite/profiles2/default-1.properties"),
            b"beta edit",
        )
        .unwrap();
        initialize(&home, &normal).unwrap();
        assert_eq!(
            fs::read(home.join(".runelite/profiles2/default-1.properties")).unwrap(),
            b"beta edit"
        );
        activate_plugins(&root, &home, &[b"one".to_vec()]).unwrap();
        activate_plugins(&root, &home, &[b"updated".to_vec(), b"two".to_vec()]).unwrap();
        assert_eq!(
            fs::read(home.join(".runelite/sideloaded-plugins/plugin-0.jar")).unwrap(),
            b"updated"
        );
        assert_eq!(
            fs::read(home.join(".runelite/profiles2/default-1.properties")).unwrap(),
            b"beta edit"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "cabbage profile failures {:x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
    #[test]
    fn invalid_copy_source_cleans_staging_and_never_alters_the_original() {
        let root = root();
        let normal = root.join("normal");
        fs::create_dir_all(normal.join(".runelite")).unwrap();
        let source = normal.join(".runelite/profiles2");
        fs::write(&source, b"keep original").unwrap();
        let home = root.join("beta");
        assert!(initialize(&home, &normal).is_err());
        assert!(!home.exists());
        assert_eq!(fs::read(source).unwrap(), b"keep original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_staging_destination_is_reported_without_overwriting_anything() {
        let root = root();
        let home = root.join("beta");
        initialize(&home, &root.join("normal")).unwrap();
        let target = home.join(".runelite/sideloaded-plugins");
        fs::write(&target, b"keep existing").unwrap();
        assert!(activate_plugins(&root, &home, &[b"plugin".to_vec()])
            .unwrap_err()
            .contains("not a regular directory"));
        assert_eq!(fs::read(target).unwrap(), b"keep existing");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn changing_plugins_while_a_managed_process_is_alive_preserves_the_old_set_and_profile() {
        let root = root();
        let home = root.join("beta");
        initialize(&home, &root.join("normal")).unwrap();
        activate_plugins(&root, &home, &[b"original".to_vec()]).unwrap();
        let settings = home.join(".runelite/settings.properties");
        fs::write(&settings, b"keep settings").unwrap();
        let mut process = std::process::Command::new("python3")
            .args(["-c", "import sys; sys.stdin.read()"])
            .arg(root.join("managed-client"))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let unchanged = activate_plugins(&root, &home, &[b"original".to_vec()]);
        let changed = activate_plugins(&root, &home, &[b"new".to_vec()]);
        let _ = process.kill();
        let _ = process.wait();
        assert!(unchanged.is_ok());
        assert!(changed.unwrap_err().contains("Close RuneLite"));
        assert_eq!(
            fs::read(home.join(".runelite/sideloaded-plugins/plugin-0.jar")).unwrap(),
            b"original"
        );
        assert_eq!(fs::read(settings).unwrap(), b"keep settings");
        activate_plugins(&root, &home, &[b"new".to_vec()]).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
