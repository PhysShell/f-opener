use crate::error::CoreError;
use crate::matching::{validate_glob, validate_regex};
use crate::types::WatchRule;

pub fn validate_rule(rule: &WatchRule) -> Result<(), CoreError> {
    if rule.name.trim().is_empty() {
        return Err(CoreError::Validation(
            "Rule name cannot be empty".to_owned(),
        ));
    }
    if rule.file_mask.trim().is_empty() {
        return Err(CoreError::Validation(
            "File mask cannot be empty".to_owned(),
        ));
    }
    validate_glob(&rule.file_mask)
        .map_err(|e| CoreError::Validation(format!("Invalid file mask: {e}")))?;
    if let Some(pattern) = &rule.regex {
        validate_regex(pattern)
            .map_err(|e| CoreError::Validation(format!("Invalid regex: {e}")))?;
    }
    if rule.stable_checks_count == 0 {
        return Err(CoreError::Validation(
            "stable_checks_count must be > 0".to_owned(),
        ));
    }
    Ok(())
}
