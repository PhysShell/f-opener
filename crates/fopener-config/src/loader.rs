use crate::schema::AppConfig;
use anyhow::{Context, Result};
use std::path::Path;

pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: AppConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))?;
    Ok(migrate(config))
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(config)
        .context("Failed to serialize config")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write config: {}", path.display()))?;
    Ok(())
}

fn migrate(config: AppConfig) -> AppConfig {
    // Future migrations go here based on config.version
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::AppConfig;
    use fopener_core::types::{ActionTemplate, WatchRule};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn sample_config() -> AppConfig {
        AppConfig {
            version: 1,
            rules: vec![WatchRule {
                id: "test-id".to_string(),
                name: "Test Rule".to_string(),
                enabled: true,
                path: PathBuf::from("/tmp/watch"),
                include_subdirectories: false,
                file_mask: "*.xml".to_string(),
                regex: None,
                action: ActionTemplate {
                    executable: PathBuf::from("editor"),
                    arguments: vec!["{file}".to_string()],
                },
                debounce_ms: 1000,
                wait_until_stable: true,
                stable_check_ms: 300,
                stable_checks_count: 3,
                ready_timeout_sec: 30,
            }],
        }
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");

        let config = sample_config();
        save_config(&path, &config).unwrap();

        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].name, "Test Rule");
        assert_eq!(loaded.rules[0].file_mask, "*.xml");
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let config = load_config(&path).unwrap();
        assert_eq!(config.version, 1);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_save_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("config.json");

        let config = AppConfig::default();
        save_config(&path, &config).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_load_invalid_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{ not valid json }").unwrap();

        let result = load_config(&path);
        assert!(result.is_err());
    }
}
