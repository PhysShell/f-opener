use fopener_core::types::WatchRule;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub rules: Vec<WatchRule>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            rules: Vec::new(),
        }
    }
}
