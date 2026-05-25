use std::path::Path;
use std::time::{Duration, Instant};
use std::{fs, thread};

use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub struct StabilityChecker {
    pub check_interval_ms: u64,
    pub required_stable_count: u32,
    pub timeout_sec: u64,
}

impl StabilityChecker {
    /// Polls the file size until it stays constant for `required_stable_count`
    /// successive samples, or until `timeout_sec` elapses.
    ///
    /// Returns `Ok(true)` on stable, `Ok(false)` on timeout.
    pub fn wait_until_stable(&self, path: &Path) -> Result<bool> {
        let timeout = Duration::from_secs(self.timeout_sec);
        let check_interval = Duration::from_millis(self.check_interval_ms);
        let deadline = Instant::now().checked_add(timeout);

        let mut stable_count: u32 = 0;
        let mut last_size: Option<u64> = None;

        loop {
            match deadline {
                Some(d) if Instant::now() >= d => return Ok(false),
                None => return Ok(false),
                Some(_) => {}
            }

            let current_size = fs::metadata(path).ok().map(|m| m.len());

            if current_size.is_some() && current_size == last_size {
                stable_count = stable_count.saturating_add(1);
                if stable_count >= self.required_stable_count {
                    return Ok(true);
                }
            } else {
                stable_count = 0;
                last_size = current_size;
            }

            thread::sleep(check_interval);
        }
    }
}
