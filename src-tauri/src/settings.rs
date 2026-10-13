use crate::clients::{ClientId, Game};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub preferences: Preferences,
    pub selected_game: Game,
    pub osrs_client: ClientId,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub minimize_on_play: bool,
    pub prefer_dedicated_gpu: bool,
    pub check_updates_on_startup: bool,
    pub include_beta_updates: bool,
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
