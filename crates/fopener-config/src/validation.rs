use crate::schema::AppConfig;
use fopener_core::validation::validate_rule;
use anyhow::Result;

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
mod tests {
    use super::*;
    use fopener_core::types::{ActionTemplate, WatchRule};
    use std::path::PathBuf;

    fn make_valid_config() -> AppConfig {
        AppConfig {
            version: 1,
            rules: vec![WatchRule {
                id: "r1".into(),
                name: "Test".into(),
                enabled: true,
                path: PathBuf::from("/watch"),
                include_subdirectories: false,
                file_mask: "*.xml".into(),
                regex: None,
                action: ActionTemplate {
                    executable: PathBuf::from("editor"),
                    arguments: vec!["{file}".into()],
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
        config.rules[0].regex = Some("[invalid".into());
        let errors = validate_config(&config).unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Invalid regex"));
    }

    #[test]
    fn test_empty_name_reported() {
        let mut config = make_valid_config();
        config.rules[0].name = "   ".into();
        let errors = validate_config(&config).unwrap();
        assert!(!errors.is_empty());
    }
}
