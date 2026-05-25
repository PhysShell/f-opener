use directories::ProjectDirs;
use std::path::PathBuf;

pub fn default_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "F-Opener").map(|dirs| dirs.config_dir().join("config.json"))
}
