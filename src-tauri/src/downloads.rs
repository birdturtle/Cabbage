use serde::Serialize;
use std::{fs, io::Write, path::Path};

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
    let bytes =
        serde_json::to_vec(value).map_err(|_| "Cannot encode client installation state.")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&staged)
            .map_err(|_| "Cannot save client installation state.")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save client installation state.")?;
        drop(file);
        // Replace in one rename. Moving the old record away first creates a crash window.
        fs::rename(&staged, root.join("current.json"))
            .map_err(|_| "Cannot activate client installation state.")?;
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_http_body_and_oversized_stream_are_rejected() {
        use std::io::Read;
        for wire in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort".as_slice(),
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n012345678901234567890123456789"
                .as_slice(),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let wire = wire.to_vec();
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                stream.read(&mut request).unwrap();
                stream.write_all(&wire).unwrap();
            });
            let result = tauri::async_runtime::block_on(async {
                let response = reqwest::Client::new()
                    .get(format!("http://{addr}/package"))
                    .send()
                    .await
                    .unwrap();
                read_limited(response, 25).await
            });
            assert!(result.is_err());
            worker.join().unwrap();
        }
    }
    #[test]
    fn failed_activation_cleans_staging_and_preserves_existing_data() {
        let root = std::env::temp_dir().join(format!(
            "cabbage record recovery {:x}",
            rand::random::<u64>()
        ));
        fs::create_dir_all(root.join("current.json")).unwrap();
        fs::write(root.join("current.json/keep"), b"previous").unwrap();
        assert!(activate_json(&root, &"new").is_err());
        assert_eq!(
            fs::read(root.join("current.json/keep")).unwrap(),
            b"previous"
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn readers_never_see_a_missing_or_partial_record_during_replacement() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let root = std::env::temp_dir().join(format!(
            "cabbage atomic recovery {:x}",
            rand::random::<u64>()
        ));
        fs::create_dir(&root).unwrap();
        activate_json(&root, &0u32).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let record = root.join("current.json");
        let reader = std::thread::spawn(move || {
            while !signal.load(Ordering::Relaxed) {
                let bytes = fs::read(&record).map_err(|_| "record disappeared")?;
                serde_json::from_slice::<u32>(&bytes).map_err(|_| "record was partial")?;
            }
            Ok::<(), &str>(())
        });
        let result = (1..=30u32).try_for_each(|value| activate_json(&root, &value));
        stop.store(true, Ordering::Relaxed);
        let observed = reader.join().unwrap();
        result.unwrap();
        observed.unwrap();
        fs::remove_dir_all(root).unwrap();
    }
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
