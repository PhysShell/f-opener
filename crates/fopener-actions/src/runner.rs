use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use fopener_core::types::WatchRule;

use crate::placeholders::render_arguments;

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
}

pub trait ActionRunner: Send + Sync {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult>;
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessActionRunner;

impl ActionRunner for ProcessActionRunner {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult> {
        let args = render_arguments(&rule.action, file, rule);

        let mut cmd = Command::new(&rule.action.executable);
        cmd.args(&args);

        // On Windows, detach from console so the parent CLI does not block.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            cmd.creation_flags(DETACHED_PROCESS);
        }

        cmd.spawn()
            .map(|_child| ActionResult {
                success: true,
                message: format!(
                    "Launched {} with args: {}",
                    rule.action.executable.display(),
                    args.join(" ")
                ),
            })
            .map_err(|e| anyhow!("Failed to launch {}: {e}", rule.action.executable.display()))
    }
}

/// Fake runner for tests — records calls without launching any process.
#[derive(Debug, Default)]
pub struct FakeActionRunner {
    pub calls: Mutex<Vec<(String, String)>>,
}

impl FakeActionRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of recorded calls, or `0` if the internal
    /// mutex has been poisoned by a panic in another thread.
    pub fn call_count(&self) -> usize {
        self.calls.lock().map(|g| g.len()).unwrap_or(0)
    }
}

impl ActionRunner for FakeActionRunner {
    fn run(&self, rule: &WatchRule, file: &Path) -> Result<ActionResult> {
        {
            let mut guard = self
                .calls
                .lock()
                .map_err(|_poisoned| anyhow!("FakeActionRunner call log mutex poisoned"))?;
            guard.push((rule.id.clone(), file.to_string_lossy().into_owned()));
        }
        Ok(ActionResult {
            success: true,
            message: format!("Fake: would open {} for rule {}", file.display(), rule.id),
        })
    }
}
