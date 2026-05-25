use std::path::Path;
use std::time::Duration;
use anyhow::Result;

pub struct StabilityChecker {
    pub check_interval_ms: u64,
    pub required_stable_count: u32,
    pub timeout_sec: u64,
}

impl StabilityChecker {
    pub fn wait_until_stable(&self, path: &Path) -> Result<bool> {
        let timeout = Duration::from_secs(self.timeout_sec);
        let check_interval = Duration::from_millis(self.check_interval_ms);
        let deadline = std::time::Instant::now() + timeout;

        let mut stable_count = 0u32;
        let mut last_size: Option<u64> = None;

        loop {
            if std::time::Instant::now() >= deadline {
                return Ok(false); // timed out
            }

            let current_size = std::fs::metadata(path)
                .ok()
                .map(|m| m.len());

            if current_size == last_size && current_size.is_some() {
                stable_count += 1;
                if stable_count >= self.required_stable_count {
                    return Ok(true);
                }
            } else {
                stable_count = 0;
                last_size = current_size;
            }

            std::thread::sleep(check_interval);
        }
    }
}
