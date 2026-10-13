use serde::Serialize;
use std::{fs, path::Path};

pub async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("Download exceeds its size limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Download was interrupted. Check your connection and retry.")?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("Download exceeds its size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub fn activate_json<T: Serialize>(root: &Path, value: &T) -> Result<(), String> {
    let staged = root.join(format!("current-{:x}.json", rand::random::<u64>()));
    fs::write(
        &staged,
        serde_json::to_vec(value).map_err(|_| "Cannot encode client installation state.")?,
    )
    .map_err(|_| "Cannot save client installation state.")?;
    let current = root.join("current.json");
    let backup = root.join(format!("previous-{:x}.json", rand::random::<u64>()));
    let had_current = current.exists();
    if had_current {
        fs::rename(&current, &backup).map_err(|_| "Cannot prepare client activation.")?;
    }
    if fs::rename(&staged, &current).is_err() {
        if had_current {
            let _ = fs::rename(&backup, &current);
        }
        return Err("Cannot activate client installation state.".into());
    }
    if had_current {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_replaces_the_record_without_leftover_backups() {
        let root =
            std::env::temp_dir().join(format!("cabbage-record-test-{:x}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        activate_json(&root, &"old").unwrap();
        activate_json(&root, &"new").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("current.json")).unwrap(),
            "\"new\""
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
