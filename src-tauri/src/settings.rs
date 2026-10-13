use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub preferences: Preferences,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub minimize_on_play: bool,
    pub check_updates_on_startup: bool,
    pub include_beta_updates: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn obsolete_component_paths_are_discarded_without_losing_preferences() {
        let settings: Settings = serde_json::from_str(r#"{"hapticscapeJar":"old.jar","lumbridgeJar":"old.jar","managedComponents":true,"preferences":{"includeBetaUpdates":true}}"#).unwrap();
        assert!(settings.preferences.include_beta_updates);
        let saved = serde_json::to_value(settings).unwrap();
        assert!(saved.get("hapticscapeJar").is_none());
        assert!(saved.get("managedComponents").is_none());
    }
}
