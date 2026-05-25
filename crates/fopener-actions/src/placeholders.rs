use fopener_core::types::{ActionTemplate, WatchRule};
use std::path::Path;

pub fn render_arguments(template: &ActionTemplate, file: &Path, rule: &WatchRule) -> Vec<String> {
    let dir = file.parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let filename = file.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = file.file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file.extension()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    template.arguments.iter().map(|arg| {
        arg.replace("{file}", &file.to_string_lossy())
            .replace("{dir}", &dir)
            .replace("{filename}", &filename)
            .replace("{stem}", &stem)
            .replace("{ext}", &ext)
            .replace("{rule}", &rule.name)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fopener_core::types::{ActionTemplate, WatchRule};
    use std::path::PathBuf;

    fn make_rule() -> WatchRule {
        WatchRule {
            id: "test".into(),
            name: "Export XML".into(),
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
        }
    }

    #[test]
    fn test_file_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{file}".into()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["/watch/export_123.xml"]);
    }

    #[test]
    fn test_stem_and_ext_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{stem}".into(), "{ext}".into()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["export_123", "xml"]);
    }

    #[test]
    fn test_rule_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{rule}".into()],
        };
        let args = render_arguments(&template, Path::new("/watch/file.xml"), &rule);
        assert_eq!(args, vec!["Export XML"]);
    }

    #[test]
    fn test_dir_placeholder() {
        let rule = make_rule();
        let template = ActionTemplate {
            executable: PathBuf::from("editor"),
            arguments: vec!["{dir}".into()],
        };
        let args = render_arguments(&template, Path::new("/watch/export_123.xml"), &rule);
        assert_eq!(args, vec!["/watch"]);
    }
}
