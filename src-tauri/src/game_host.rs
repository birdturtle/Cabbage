use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const BYTES: &[u8] = include_bytes!(env!("CABBAGE_GAME_HOST"));

pub fn install(root: &Path) -> Result<PathBuf, String> {
    let digest = format!("{:x}", Sha256::digest(BYTES));
    let folder = root.join("support").join(&digest);
    fs::create_dir_all(&folder).map_err(|_| "Cannot create game helper directory.")?;
    let name = if cfg!(windows) {
        "cabbage-game-host.exe"
    } else {
        "cabbage-game-host"
    };
    let path = folder.join(name);
    if path.exists() {
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file())
            && fs::read(&path).is_ok_and(|bytes| bytes == BYTES)
        {
            return Ok(path);
        }
        return Err("Game helper files are damaged. Repair the launcher installation.".into());
    }
    let staged = folder.join(format!(".stage-{:x}", rand::random::<u64>()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o700);
    }
    let result = (|| {
        let mut file = options
            .open(&staged)
            .map_err(|_| "Cannot save game helper.")?;
        file.write_all(BYTES)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save game helper.")?;
        drop(file);
        // A hard link publishes without replacing an existing, possibly active helper.
        match fs::hard_link(&staged, &path) {
            Ok(()) => Ok(path.clone()),
            Err(_) if fs::read(&path).is_ok_and(|bytes| bytes == BYTES) => Ok(path.clone()),
            Err(_) => Err("Cannot install game helper.".into()),
        }
    })();
    let _ = fs::remove_file(staged);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_is_verified_reused_and_never_overwritten() {
        let root = std::env::temp_dir().join(format!("cabbage-helper-{:x}", rand::random::<u64>()));
        let first = install(&root).unwrap();
        assert_eq!(first, install(&root).unwrap());
        fs::write(&first, b"damaged").unwrap();
        assert!(install(&root).is_err());
        assert_eq!(fs::read(first).unwrap(), b"damaged");
        fs::remove_dir_all(root).unwrap();
    }
}
