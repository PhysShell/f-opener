use std::path::PathBuf;
use directories::ProjectDirs;

pub fn default_config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "F-Opener").map(|dirs| {
        dirs.config_dir().join("config.json")
    })
}
