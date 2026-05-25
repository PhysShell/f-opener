use std::path::Path;

use fopener_core::types::{ActionTemplate, WatchRule};

/// Render argument templates with placeholders substituted.
///
/// Recognised tokens (intentionally formatted as `{name}` so they read
/// like format-string args even though they are plain text): `{file}`,
/// `{dir}`, `{filename}`, `{stem}`, `{ext}`, `{rule}`.
#[allow(
    clippy::literal_string_with_formatting_args,
    reason = "tokens look like format args but are user-facing placeholders"
)]
pub fn render_arguments(template: &ActionTemplate, file: &Path, rule: &WatchRule) -> Vec<String> {
    let dir = file
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = file
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = file
        .extension()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    template
        .arguments
        .iter()
        .map(|arg| {
            arg.replace("{file}", &file.to_string_lossy())
                .replace("{dir}", &dir)
                .replace("{filename}", &filename)
                .replace("{stem}", &stem)
                .replace("{ext}", &ext)
                .replace("{rule}", &rule.name)
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::literal_string_with_formatting_args,
    reason = "tests assert invariants; placeholder tokens look like format args"
)]
mod tests {
    use std::path::PathBuf;

    use fopener_core::types::{ActionTemplate, WatchRule};

    use super::*;

    fn make_rule() -> WatchRule {
        WatchRule {
            id: "test".to_owned(),
            name: "Export XML".to_owned(),
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
        }
    }

    #[test]
    fn test_file_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{file}".to_owned()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["/watch/export_123.xml"]);
    }

    #[test]
    fn test_stem_and_ext_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{stem}".to_owned(), "{ext}".to_owned()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["export_123", "xml"]);
    }

    #[test]
    fn test_rule_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{rule}".to_owned()],
        };
        let args = render_arguments(&template, Path::new("/watch/file.xml"), &rule);
        assert_eq!(args, vec!["Export XML"]);
    }

    #[test]
    fn test_dir_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{dir}".to_owned()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["/watch"]);
    }
}
