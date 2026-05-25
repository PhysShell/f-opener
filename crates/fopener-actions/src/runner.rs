use fopener_core::types::WatchRule;
use anyhow::Result;
use std::path::Path;
use std::process::Command;
use crate::placeholders::render_arguments;

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
}

pub trait ActionRunner: Send + Sync {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult>;
}

pub struct ProcessActionRunner;

impl ActionRunner for ProcessActionRunner {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult> {
        let args = render_arguments(&rule.action, file, rule);

        let mut cmd = Command::new(&rule.action.executable);
        cmd.args(&args);

        // On Windows, detach from console
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x00000008); // DETACHED_PROCESS
        }

        cmd.spawn()
            .map(|_| ActionResult {
                success: true,
                message: format!(
                    "Launched {} with args: {}",
                    rule.action.executable.display(),
                    args.join(" ")
                ),
            })
            .map_err(|e| anyhow::anyhow!(
                "Failed to launch {}: {}",
                rule.action.executable.display(),
                e
            ))
    }
}

/// Fake runner for tests — records calls without launching anything
pub struct FakeActionRunner {
    pub calls: std::sync::Mutex<Vec<(String, String)>>, // (rule_id, file_path)
}

impl FakeActionRunner {
    pub fn new() -> Self {
        Self { calls: std::sync::Mutex::new(Vec::new()) }
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Default for FakeActionRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRunner for FakeActionRunner {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult> {
        self.calls.lock().unwrap().push((
            rule.id.clone(),
            file.to_string_lossy().to_string(),
        ));
        Ok(ActionResult {
            success: true,
            message: format!("Fake: would open {} for rule {}", file.display(), rule.id),
        })
    }
}
