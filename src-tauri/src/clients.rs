use crate::{osrs, rs3, runelite};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Game {
    #[default]
    Oldschool,
    Runescape,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientId {
    #[default]
    Runelite,
    OfficialOsrs,
    OfficialRs3,
}
impl ClientId {
    pub fn game(self) -> Game {
        match self {
            Self::Runelite | Self::OfficialOsrs => Game::Oldschool,
            Self::OfficialRs3 => Game::Runescape,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Runelite => "RuneLite",
            Self::OfficialOsrs | Self::OfficialRs3 => "Official",
        }
    }
    pub fn directory(self) -> &'static str {
        match self {
            Self::Runelite => "runelite",
            Self::OfficialOsrs => "official-osrs",
            Self::OfficialRs3 => "official-rs3",
        }
    }
    pub fn unavailable_reason(self, os: &str, arch: &str) -> Option<&'static str> {
        if !matches!(os, "windows" | "linux") || !matches!(arch, "x86_64" | "aarch64") {
            return Some("This platform is not supported.");
        }
        match self {
            Self::Runelite => None,
            Self::OfficialOsrs if arch != "x86_64" => {
                Some("The official Old School client currently supports x86_64 only.")
            }
            Self::OfficialOsrs if os == "linux" && osrs::wine_path().is_none() => {
                Some("Install Wine to use the official Old School client on Linux.")
            }
            Self::OfficialOsrs => None,
            Self::OfficialRs3 if arch != "x86_64" => {
                Some("The RuneScape client currently supports x86_64 only.")
            }
            Self::OfficialRs3 => None,
        }
    }
    pub async fn prepare(
        self,
        root: &Path,
        progress: &Mutex<Progress>,
    ) -> Result<PreparedClient, String> {
        if let Some(reason) = self.unavailable_reason(std::env::consts::OS, std::env::consts::ARCH)
        {
            return Err(reason.into());
        }
        match self {
            Self::Runelite => {
                let root = root.join(self.directory());
                let install = runelite::ensure(&root, progress).await?;
                Ok(PreparedClient::RuneLite { root, install })
            }
            Self::OfficialOsrs => {
                let root = root.join(self.directory());
                let install = osrs::ensure(&root, progress).await?;
                osrs::prepare_wine(&root, progress)?;
                Ok(PreparedClient::OfficialOsrs { root, install })
            }
            Self::OfficialRs3 => {
                let root = root.join(self.directory());
                let prepared = rs3::ensure(&root, progress).await?;
                Ok(PreparedClient::OfficialRs3 { root, prepared })
            }
        }
    }
    pub fn installed_version(self, root: &Path) -> Option<String> {
        match self {
            Self::Runelite => runelite::installed(&root.join(self.directory())).map(|i| i.version),
            Self::OfficialOsrs => osrs::installed(&root.join(self.directory())).map(|i| i.version),
            Self::OfficialRs3 => rs3::installed(&root.join(self.directory())).map(|i| i.version),
        }
    }
}
pub enum PreparedClient {
    OfficialRs3 {
        root: PathBuf,
        prepared: rs3::Prepared,
    },
    OfficialOsrs {
        root: PathBuf,
        install: osrs::Installed,
    },
    RuneLite {
        root: PathBuf,
        install: runelite::Installed,
    },
}
impl PreparedClient {
    pub fn spawn(
        self,
        credentials: &[(String, String)],
        prefer_gpu: bool,
    ) -> Result<Child, String> {
        match self {
            Self::RuneLite { root, install } => {
                runelite::spawn(&root, &install, credentials, prefer_gpu)
            }
            Self::OfficialOsrs { root, install } => {
                osrs::spawn(&root, &install, credentials, prefer_gpu)
            }
            Self::OfficialRs3 { root, prepared } => {
                rs3::spawn(&root, prepared, credentials, prefer_gpu)
            }
        }
    }
}
pub fn configure_credentials(command: &mut Command, credentials: &[(String, String)]) {
    for key in [
        "JX_SESSION_ID",
        "JX_CHARACTER_ID",
        "JX_DISPLAY_NAME",
        "JX_ACCESS_TOKEN",
        "JX_REFRESH_TOKEN",
    ] {
        command.env_remove(key);
    }
    for (key, value) in credentials {
        if matches!(
            key.as_str(),
            "JX_SESSION_ID" | "JX_CHARACTER_ID" | "JX_DISPLAY_NAME"
        ) {
            command.env(key, value);
        }
    }
}
#[derive(Default)]
pub struct Progress {
    pub busy: bool,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LaunchKey {
    pub account: String,
    pub character: String,
}
struct Running {
    client: ClientId,
    child: Child,
    name: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub id: ClientId,
    pub game: Game,
    pub name: &'static str,
    pub available: bool,
    pub unavailable_reason: Option<&'static str>,
}
pub fn registry() -> Vec<Descriptor> {
    [
        ClientId::Runelite,
        ClientId::OfficialOsrs,
        ClientId::OfficialRs3,
    ]
    .into_iter()
    .map(|id| {
        let unavailable_reason =
            id.unavailable_reason(std::env::consts::OS, std::env::consts::ARCH);
        Descriptor {
            id,
            game: id.game(),
            name: id.name(),
            available: unavailable_reason.is_none(),
            unavailable_reason,
        }
    })
    .collect()
}
#[derive(Default)]
pub struct Manager {
    progress: HashMap<ClientId, Arc<Mutex<Progress>>>,
    preparing: HashSet<LaunchKey>,
    running: HashMap<LaunchKey, Running>,
}
impl Manager {
    pub fn progress(&mut self, client: ClientId) -> Arc<Mutex<Progress>> {
        self.progress.entry(client).or_default().clone()
    }
    pub fn reap(&mut self) {
        let mut ended = Vec::new();
        self.running
            .retain(|_, running| match running.child.try_wait() {
                Ok(None) => true,
                Ok(Some(exit)) => {
                    ended.push((
                        running.client,
                        if exit.success() {
                            format!("{} closed.", running.client.name())
                        } else {
                            format!(
                                "{} exited unexpectedly. Check its launch log.",
                                running.client.name()
                            )
                        },
                    ));
                    false
                }
                Err(_) => true,
            });
        for (client, message) in ended {
            self.progress(client).lock().unwrap().message = message;
        }
    }
    pub fn is_running(&self, key: &LaunchKey) -> bool {
        self.running.contains_key(key)
    }
    pub fn running_message(&self, key: &LaunchKey) -> Option<String> {
        self.running
            .get(key)
            .map(|r| format!("{} is running for {}.", r.client.name(), r.name))
    }
    pub fn begin(
        &mut self,
        client: ClientId,
        key: &LaunchKey,
    ) -> Result<Arc<Mutex<Progress>>, String> {
        self.reap();
        if let Some(reason) =
            client.unavailable_reason(std::env::consts::OS, std::env::consts::ARCH)
        {
            return Err(reason.into());
        }
        if self.running.contains_key(key) || self.preparing.contains(key) {
            return Err("This character is already running or starting.".into());
        }
        let progress = self.progress(client);
        {
            let mut state = progress.lock().unwrap();
            if state.busy {
                return Err(
                    "This client is already being prepared. Try again when it is ready.".into(),
                );
            }
            state.busy = true;
            state.message = format!("Preparing {}...", client.name());
        }
        self.preparing.insert(key.clone());
        Ok(progress)
    }
    pub fn started(&mut self, client: ClientId, key: LaunchKey, name: String, child: Child) {
        self.running.insert(
            key,
            Running {
                client,
                name,
                child,
            },
        );
    }
    pub fn finish(&mut self, client: ClientId, key: &LaunchKey, result: &Result<(), String>) {
        self.preparing.remove(key);
        let progress = self.progress(client);
        let mut state = progress.lock().unwrap();
        state.busy = false;
        state.message = match result {
            Ok(()) => String::new(),
            Err(error) => error.clone(),
        };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(character: &str) -> LaunchKey {
        LaunchKey {
            account: "account".into(),
            character: character.into(),
        }
    }
    fn sleeping_child() -> Child {
        if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap()
        }
    }
    #[test]
    fn unavailable_adapters_cannot_prepare_or_launch() {
        assert!(ClientId::OfficialRs3
            .unavailable_reason("linux", "aarch64")
            .is_some());
        assert!(ClientId::OfficialRs3
            .unavailable_reason("macos", "x86_64")
            .is_some());
        assert!(ClientId::OfficialRs3
            .unavailable_reason("linux", "x86_64")
            .is_none());
    }
    #[test]
    fn duplicate_reservations_are_blocked_and_failures_release_them() {
        let mut manager = Manager::default();
        manager.begin(ClientId::Runelite, &key("a")).unwrap();
        assert!(manager.begin(ClientId::Runelite, &key("a")).is_err());
        manager.finish(
            ClientId::Runelite,
            &key("a"),
            &Err("Download failed".into()),
        );
        manager.begin(ClientId::Runelite, &key("a")).unwrap();
    }
    #[test]
    fn different_characters_can_run_without_replacing_each_other() {
        let mut manager = Manager::default();
        for character in ["a", "b"] {
            manager.begin(ClientId::Runelite, &key(character)).unwrap();
            manager.started(
                ClientId::Runelite,
                key(character),
                character.into(),
                sleeping_child(),
            );
            manager.finish(ClientId::Runelite, &key(character), &Ok(()));
        }
        assert!(manager.begin(ClientId::Runelite, &key("a")).is_err());
        assert_eq!(manager.running.len(), 2);
        for running in manager.running.values_mut() {
            running.child.kill().unwrap();
            running.child.wait().unwrap();
        }
        manager.reap();
        assert!(manager.running.is_empty());
    }
    #[test]
    fn installation_paths_keep_the_existing_runelite_directory() {
        assert_eq!(
            Path::new("clients").join(ClientId::Runelite.directory()),
            Path::new("clients/runelite")
        );
        assert_ne!(
            ClientId::Runelite.directory(),
            ClientId::OfficialOsrs.directory()
        );
    }
}
