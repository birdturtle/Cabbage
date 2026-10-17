use crate::clients::{ClientId, Game};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub preferences: Preferences,
    pub selected_game: Game,
    pub osrs_client: ClientId,
    pub developer_plugins: crate::developer_plugins::DeveloperPlugins,
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AfterPlay {
    #[default]
    KeepOpen,
    HideToTray,
    CloseLauncher,
}

#[derive(Clone, Default, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub after_play: AfterPlay,
    pub prefer_dedicated_gpu: bool,
    pub check_updates_on_startup: bool,
    pub include_beta_updates: bool,
}

impl<'de> Deserialize<'de> for Preferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        #[serde(rename_all = "camelCase", default)]
        struct Saved {
            after_play: Option<AfterPlay>,
            minimize_on_play: bool,
            prefer_dedicated_gpu: bool,
            check_updates_on_startup: bool,
            include_beta_updates: bool,
        }
        let saved = Saved::deserialize(deserializer)?;
        Ok(Self {
            after_play: saved.after_play.unwrap_or(if saved.minimize_on_play {
                AfterPlay::HideToTray
            } else {
                AfterPlay::KeepOpen
            }),
            prefer_dedicated_gpu: saved.prefer_dedicated_gpu,
            check_updates_on_startup: saved.check_updates_on_startup,
            include_beta_updates: saved.include_beta_updates,
        })
    }
}

impl Settings {
    pub fn selected_client(&self) -> ClientId {
        match self.selected_game {
            Game::Runescape => ClientId::OfficialRs3,
            Game::Oldschool if self.osrs_client.game() == Game::Oldschool => self.osrs_client,
            Game::Oldschool => ClientId::Runelite,
        }
    }
    pub fn select(&mut self, game: Game, client: ClientId) -> Result<(), String> {
        if client.game() != game {
            return Err("This client does not belong to the selected game.".into());
        }
        self.selected_game = game;
        if game == Game::Oldschool {
            self.osrs_client = client;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn after_play_migrates_legacy_preferences_and_preserves_explicit_choices() {
        let tray: Preferences =
            serde_json::from_str(r#"{"minimizeOnPlay":true,"preferDedicatedGpu":true}"#).unwrap();
        assert_eq!(tray.after_play, AfterPlay::HideToTray);
        assert!(tray.prefer_dedicated_gpu);
        let keep: Preferences = serde_json::from_str(r#"{"minimizeOnPlay":false}"#).unwrap();
        assert_eq!(keep.after_play, AfterPlay::KeepOpen);
        let close: Preferences =
            serde_json::from_str(r#"{"afterPlay":"closeLauncher","minimizeOnPlay":true}"#).unwrap();
        assert_eq!(close.after_play, AfterPlay::CloseLauncher);
        for choice in [
            AfterPlay::KeepOpen,
            AfterPlay::HideToTray,
            AfterPlay::CloseLauncher,
        ] {
            let preferences = Preferences {
                after_play: choice,
                ..Default::default()
            };
            let saved = serde_json::to_string(&preferences).unwrap();
            assert!(!saved.contains("minimizeOnPlay"));
            assert_eq!(
                serde_json::from_str::<Preferences>(&saved)
                    .unwrap()
                    .after_play,
                choice
            );
        }
    }
    #[test]
    fn game_switching_remembers_osrs_choice_and_rejects_mismatched_clients() {
        let mut settings = Settings::default();
        settings
            .select(Game::Oldschool, ClientId::OfficialOsrs)
            .unwrap();
        settings
            .select(Game::Runescape, ClientId::OfficialRs3)
            .unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        let mut restored: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.osrs_client, ClientId::OfficialOsrs);
        assert!(restored
            .select(Game::Runescape, ClientId::Runelite)
            .is_err());
        restored.selected_game = Game::Oldschool;
        assert_eq!(restored.selected_client(), ClientId::OfficialOsrs);
    }
    #[test]
    fn gpu_preference_survives_settings_round_trip() {
        let mut settings = Settings::default();
        settings.preferences.prefer_dedicated_gpu = true;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(restored.preferences.prefer_dedicated_gpu);
    }
    #[test]
    fn obsolete_component_paths_are_discarded_without_losing_preferences() {
        let settings: Settings = serde_json::from_str(r#"{"hapticscapeJar":"old.jar","lumbridgeJar":"old.jar","managedComponents":true,"preferences":{"includeBetaUpdates":true}}"#).unwrap();
        assert!(settings.preferences.include_beta_updates);
        assert!(!settings.preferences.prefer_dedicated_gpu);
        assert_eq!(settings.selected_client(), ClientId::Runelite);
        let saved = serde_json::to_value(settings).unwrap();
        assert!(saved.get("hapticscapeJar").is_none());
        assert!(saved.get("managedComponents").is_none());
    }
}
