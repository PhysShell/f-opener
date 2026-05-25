use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use fopener_core::matching::RuleMatcher;
use fopener_core::types::{AppEvent, FileCandidate, MatchDecision, WatchRule};
use fopener_actions::runner::ActionRunner;
use notify::{RecursiveMode, Watcher};
use tracing::{error, info, warn};

use crate::stable::StabilityChecker;

pub struct WatcherEngine {
    rules: Vec<WatchRule>,
    action_runner: Arc<dyn ActionRunner>,
    event_tx: std::sync::mpsc::Sender<AppEvent>,
}

impl WatcherEngine {
    pub fn new(
        rules: Vec<WatchRule>,
        action_runner: Arc<dyn ActionRunner>,
        event_tx: std::sync::mpsc::Sender<AppEvent>,
    ) -> Self {
        Self { rules, action_runner, event_tx }
    }

    pub fn run(self) -> Result<WatcherHandle> {
        let rules = Arc::new(self.rules);
        let action_runner = self.action_runner;
        let event_tx = self.event_tx;

        let (fs_tx, fs_rx) = std::sync::mpsc::channel::<notify::Result<Vec<notify::Event>>>();

        // Build matchers — validate all rules before starting
        let matchers: Vec<_> = rules.iter().map(|r| {
            RuleMatcher::new(r).expect("rule was validated before starting watcher")
        }).collect();

        // Collect unique watch paths
        let watch_paths: Vec<(PathBuf, bool)> = rules.iter()
            .map(|r| (r.path.clone(), r.include_subdirectories))
            .collect();

        // Channel for stopping
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

        let rules_clone = Arc::clone(&rules);
        let event_tx_clone = event_tx.clone();

        let thread = thread::spawn(move || {
            let fs_tx_inner = fs_tx.clone();
            let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                match res {
                    Ok(event) => { let _ = fs_tx_inner.send(Ok(vec![event])); }
                    Err(e) => { let _ = fs_tx_inner.send(Err(e)); }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    let _ = event_tx_clone.send(AppEvent::Error {
                        message: format!("Failed to create watcher: {e}"),
                    });
                    return;
                }
            };

            for (path, recursive) in &watch_paths {
                let mode = if *recursive { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
                if let Err(e) = watcher.watch(path, mode) {
                    let _ = event_tx_clone.send(AppEvent::Warning {
                        message: format!("Failed to watch {}: {e}", path.display()),
                    });
                }
            }

            // Notify rules started
            for rule in rules_clone.iter() {
                let _ = event_tx_clone.send(AppEvent::RuleStarted { rule_id: rule.id.clone() });
                info!("Rule started: {}", rule.id);
            }

            loop {
                // Check for stop signal (non-blocking)
                if stop_rx.try_recv().is_ok() {
                    break;
                }

                // Process fs events with timeout
                match fs_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(events)) => {
                        for event in events {
                            process_event(
                                &event,
                                &rules_clone,
                                &matchers,
                                &action_runner,
                                &event_tx_clone,
                            );
                        }
                    }
                    Ok(Err(e)) => {
                        let _ = event_tx_clone.send(AppEvent::Warning {
                            message: format!("Watcher error: {e}"),
                        });
                        warn!("Watcher error: {e}");
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            for rule in rules_clone.iter() {
                let _ = event_tx_clone.send(AppEvent::RuleStopped { rule_id: rule.id.clone() });
                info!("Rule stopped: {}", rule.id);
            }

            // Keep watcher alive until here so it doesn't drop early
            drop(watcher);
        });

        Ok(WatcherHandle { stop_tx, thread: Some(thread) })
    }
}

fn process_event(
    event: &notify::Event,
    rules: &[WatchRule],
    matchers: &[RuleMatcher],
    action_runner: &Arc<dyn ActionRunner>,
    event_tx: &std::sync::mpsc::Sender<AppEvent>,
) {
    use notify::EventKind;

    // Only care about create and modify events
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {}
        _ => return,
    }

    for path in &event.paths {
        if path.is_dir() { continue; }

        let candidate = FileCandidate::from_path(path.clone());

        for (rule, matcher) in rules.iter().zip(matchers.iter()) {
            let _ = event_tx.send(AppEvent::FileDetected {
                rule_id: rule.id.clone(),
                path: path.clone(),
            });

            match matcher.decide(rule, &candidate) {
                MatchDecision::Matched => {
                    let _ = event_tx.send(AppEvent::FileMatched {
                        rule_id: rule.id.clone(),
                        path: path.clone(),
                    });

                    // Wait until stable if configured
                    if rule.wait_until_stable {
                        let checker = StabilityChecker {
                            check_interval_ms: rule.stable_check_ms,
                            required_stable_count: rule.stable_checks_count,
                            timeout_sec: rule.ready_timeout_sec,
                        };
                        match checker.wait_until_stable(path) {
                            Ok(true) => {}
                            Ok(false) => {
                                let _ = event_tx.send(AppEvent::Warning {
                                    message: format!(
                                        "Timeout waiting for file to stabilize: {}",
                                        path.display()
                                    ),
                                });
                                continue;
                            }
                            Err(e) => {
                                let _ = event_tx.send(AppEvent::Error {
                                    message: format!("Stability check error: {e}"),
                                });
                                continue;
                            }
                        }
                    }

                    let _ = event_tx.send(AppEvent::FileReady {
                        rule_id: rule.id.clone(),
                        path: path.clone(),
                    });

                    let _ = event_tx.send(AppEvent::ActionStarted {
                        rule_id: rule.id.clone(),
                        executable: rule.action.executable.clone(),
                        arguments: rule.action.arguments.clone(),
                    });

                    match action_runner.run(rule, path) {
                        Ok(_) => {
                            let _ = event_tx.send(AppEvent::ActionCompleted {
                                rule_id: rule.id.clone(),
                                path: path.clone(),
                            });
                        }
                        Err(e) => {
                            error!("Action failed for rule {}: {e}", rule.id);
                            let _ = event_tx.send(AppEvent::ActionFailed {
                                rule_id: rule.id.clone(),
                                path: path.clone(),
                                error: e.to_string(),
                            });
                        }
                    }
                }
                MatchDecision::Ignored { reason } => {
                    let _ = event_tx.send(AppEvent::FileIgnored {
                        rule_id: rule.id.clone(),
                        path: path.clone(),
                        reason: reason.to_string(),
                    });
                }
            }
        }
    }
}

pub struct WatcherHandle {
    stop_tx: std::sync::mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WatcherHandle {
    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
