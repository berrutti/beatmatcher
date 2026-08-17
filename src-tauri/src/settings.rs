use std::path::PathBuf;

// tauri-plugin-store resolves a bare filename against BaseDirectory::AppData,
// which is `dirs::data_dir()/<bundle identifier>`.
const BUNDLE_IDENTIFIER: &str = "com.berrutti.beatmatcher";
const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "v1";

pub fn store_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(BUNDLE_IDENTIFIER).join(STORE_FILE))
}

/// None when nothing readable is stored, leaving the default to the caller.
pub fn limiter_enabled() -> Option<bool> {
    let contents = std::fs::read_to_string(store_path()?).ok()?;
    read_limiter_enabled(&contents)
}

fn read_limiter_enabled(contents: &str) -> Option<bool> {
    let stored: serde_json::Value = serde_json::from_str(contents).ok()?;
    stored[STORE_KEY]["limiterEnabled"].as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_limiter_setting_is_read() {
        assert_eq!(
            read_limiter_enabled(r#"{"v1":{"limiterEnabled":false,"bpmMin":90}}"#),
            Some(false)
        );
        assert_eq!(
            read_limiter_enabled(r#"{"v1":{"limiterEnabled":true}}"#),
            Some(true)
        );
    }

    #[test]
    fn an_unwritten_setting_is_not_a_value() {
        assert_eq!(read_limiter_enabled(r#"{"v1":{"bpmMin":90}}"#), None);
        assert_eq!(
            read_limiter_enabled(r#"{"v2":{"limiterEnabled":false}}"#),
            None
        );
        assert_eq!(read_limiter_enabled("{}"), None);
        assert_eq!(read_limiter_enabled("not json"), None);
        assert_eq!(
            read_limiter_enabled(r#"{"v1":{"limiterEnabled":"off"}}"#),
            None
        );
    }

    #[test]
    fn the_bundle_identifier_matches_the_tauri_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("parse tauri.conf.json");
        assert_eq!(config["identifier"].as_str(), Some(BUNDLE_IDENTIFIER));
    }
}
