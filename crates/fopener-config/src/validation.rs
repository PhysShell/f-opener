use anyhow::Result;
use fopener_core::validation::validate_rule;

use crate::schema::AppConfig;

pub fn validate_config(config: &AppConfig) -> Result<Vec<String>> {
    let mut errors = Vec::new();
    for rule in &config.rules {
        if let Err(e) = validate_rule(rule) {
            errors.push(format!("Rule '{}': {}", rule.name, e));
        }
    }
    Ok(errors)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::shadow_unrelated,
    reason = "tests assert invariants; concise unwraps are appropriate"
)]
mod tests {
    use std::path::PathBuf;

    use fopener_core::types::{ActionTemplate, WatchRule};

    use super::*;

    fn make_valid_config() -> AppConfig {
        AppConfig {
            version: 1,
            rules: vec![WatchRule {
                id: "r1".to_owned(),
                name: "Test".to_owned(),
                enabled: true,
                path: PathBuf::from("/watch"),
                include_subdirectories: false,
                file_mask: "*.xml".to_owned(),
                regex: None,
                action: ActionTemplate {
                    executable: PathBuf::from("editor"),
                    arguments: vec!["{file}".to_owned()],
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
    fn test_valid_config_no_errors() {
        let config = make_valid_config();
        let errors = validate_config(&config).unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_invalid_regex_reported() {
        let mut config = make_valid_config();
        config.rules[0].regex = Some("[invalid".to_owned());
        let errors = validate_config(&config).unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Invalid regex"));
    }

    #[test]
    fn test_empty_name_reported() {
        let mut config = make_valid_config();
        config.rules[0].name = "   ".to_owned();
        let errors = validate_config(&config).unwrap();
        assert!(!errors.is_empty());
    }
}
