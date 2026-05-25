use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use fopener_actions::runner::ActionRunner;
use fopener_core::matching::RuleMatcher;
use fopener_core::types::{AppEvent, FileCandidate, MatchDecision, WatchRule};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use tracing::{error, info, warn};

use crate::stable::StabilityChecker;

#[allow(
    missing_debug_implementations,
    reason = "holds trait objects with no Debug bound"
)]
pub struct WatcherEngine {
    rules: Vec<WatchRule>,
    action_runner: Arc<dyn ActionRunner>,
    event_tx: Sender<AppEvent>,
}

/// Send an [`AppEvent`] on a best-effort basis; closed channels are
/// not fatal for the watcher.
#[allow(
    let_underscore_drop,
    clippy::let_underscore_must_use,
    reason = "consumer disconnects are recovered by exiting the watch loop"
)]
fn fire(tx: &Sender<AppEvent>, evt: AppEvent) {
    let _ = tx.send(evt);
}

impl WatcherEngine {
    pub fn new(
        rules: Vec<WatchRule>,
        action_runner: Arc<dyn ActionRunner>,
        event_tx: Sender<AppEvent>,
    ) -> Self {
        Self {
            rules,
            action_runner,
            event_tx,
        }
    }

    pub fn run(self) -> Result<WatcherHandle> {
        let Self {
            rules,
            action_runner,
            event_tx,
        } = self;

        let matchers = rules
            .iter()
            .map(RuleMatcher::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("rule matcher build failed: {e}"))?;

        let watch_paths: Vec<(PathBuf, bool)> = rules
            .iter()
            .map(|r| (r.path.clone(), r.include_subdirectories))
            .collect();

        let rules: Arc<[WatchRule]> = Arc::from(rules);
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let thread = thread::spawn(move || {
            let ctx = WatchLoopContext {
                rules: &rules,
                matchers: &matchers,
                watch_paths: &watch_paths,
                action_runner: &action_runner,
                event_tx: &event_tx,
                stop_rx: &stop_rx,
            };
            run_watch_loop(ctx);
        });

        Ok(WatcherHandle {
            stop_tx,
            thread: Some(thread),
        })
    }
}

#[derive(Clone, Copy)]
struct WatchLoopContext<'a> {
    rules: &'a [WatchRule],
    matchers: &'a [RuleMatcher],
    watch_paths: &'a [(PathBuf, bool)],
    action_runner: &'a Arc<dyn ActionRunner>,
    event_tx: &'a Sender<AppEvent>,
    stop_rx: &'a Receiver<()>,
}

#[allow(
    clippy::too_many_lines,
    reason = "single loop body is clearer than splitting setup/teardown across helpers"
)]
fn run_watch_loop(ctx: WatchLoopContext<'_>) {
    let (fs_tx, fs_rx) = mpsc::channel::<notify::Result<Vec<Event>>>();

    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        let payload = res.map(|event| vec![event]);
        // Receiver only disconnects during shutdown; nothing to do.
        drop(fs_tx.send(payload));
    }) {
        Ok(w) => w,
        Err(e) => {
            fire(
                ctx.event_tx,
                AppEvent::Error {
                    message: format!("Failed to create watcher: {e}"),
                },
            );
            return;
        }
    };

    for (path, recursive) in ctx.watch_paths {
        let mode = if *recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if let Err(e) = watcher.watch(path, mode) {
            fire(
                ctx.event_tx,
                AppEvent::Warning {
                    message: format!("Failed to watch {}: {e}", path.display()),
                },
            );
        }
    }

    for rule in ctx.rules {
        fire(
            ctx.event_tx,
            AppEvent::RuleStarted {
                rule_id: rule.id.clone(),
            },
        );
        info!("Rule started: {}", rule.id);
    }

    loop {
        if ctx.stop_rx.try_recv().is_ok() {
            break;
        }

        match fs_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(events)) => {
                for event in events {
                    process_event(
                        &event,
                        ctx.rules,
                        ctx.matchers,
                        ctx.action_runner,
                        ctx.event_tx,
                    );
                }
            }
            Ok(Err(e)) => {
                fire(
                    ctx.event_tx,
                    AppEvent::Warning {
                        message: format!("Watcher error: {e}"),
                    },
                );
                warn!("Watcher error: {e}");
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    for rule in ctx.rules {
        fire(
            ctx.event_tx,
            AppEvent::RuleStopped {
                rule_id: rule.id.clone(),
            },
        );
        info!("Rule stopped: {}", rule.id);
    }

    drop(watcher);
}

fn process_event(
    event: &Event,
    rules: &[WatchRule],
    matchers: &[RuleMatcher],
    action_runner: &Arc<dyn ActionRunner>,
    event_tx: &Sender<AppEvent>,
) {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return;
    }

    for path in &event.paths {
        if path.is_dir() {
            continue;
        }
        let candidate = FileCandidate::from_path(path.clone());
        dispatch_candidate(&candidate, path, rules, matchers, action_runner, event_tx);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "decision context is small and explicit; bundling adds indirection"
)]
fn dispatch_candidate(
    candidate: &FileCandidate,
    path: &Path,
    rules: &[WatchRule],
    matchers: &[RuleMatcher],
    action_runner: &Arc<dyn ActionRunner>,
    event_tx: &Sender<AppEvent>,
) {
    for (rule, matcher) in rules.iter().zip(matchers.iter()) {
        fire(
            event_tx,
            AppEvent::FileDetected {
                rule_id: rule.id.clone(),
                path: path.to_path_buf(),
            },
        );

        match matcher.decide(rule, candidate) {
            MatchDecision::Matched => {
                handle_match(rule, path, action_runner, event_tx);
            }
            MatchDecision::Ignored { reason } => {
                fire(
                    event_tx,
                    AppEvent::FileIgnored {
                        rule_id: rule.id.clone(),
                        path: path.to_path_buf(),
                        reason: reason.to_string(),
                    },
                );
            }
        }
    }
}

fn handle_match(
    rule: &WatchRule,
    path: &Path,
    action_runner: &Arc<dyn ActionRunner>,
    event_tx: &Sender<AppEvent>,
) {
    fire(
        event_tx,
        AppEvent::FileMatched {
            rule_id: rule.id.clone(),
            path: path.to_path_buf(),
        },
    );

    if rule.wait_until_stable && !wait_stable(rule, path, event_tx) {
        return;
    }

    fire(
        event_tx,
        AppEvent::FileReady {
            rule_id: rule.id.clone(),
            path: path.to_path_buf(),
        },
    );
    fire(
        event_tx,
        AppEvent::ActionStarted {
            rule_id: rule.id.clone(),
            executable: rule.action.executable.clone(),
            arguments: rule.action.arguments.clone(),
        },
    );

    match action_runner.run(rule, path) {
        Ok(_result) => {
            fire(
                event_tx,
                AppEvent::ActionCompleted {
                    rule_id: rule.id.clone(),
                    path: path.to_path_buf(),
                },
            );
        }
        Err(e) => {
            error!("Action failed for rule {}: {e}", rule.id);
            fire(
                event_tx,
                AppEvent::ActionFailed {
                    rule_id: rule.id.clone(),
                    path: path.to_path_buf(),
                    error: e.to_string(),
                },
            );
        }
    }
}

/// Waits for the file to stabilise. Returns `true` on success, `false`
/// when the wait should be abandoned (timeout or error already reported).
fn wait_stable(rule: &WatchRule, path: &Path, event_tx: &Sender<AppEvent>) -> bool {
    let checker = StabilityChecker {
        check_interval_ms: rule.stable_check_ms,
        required_stable_count: rule.stable_checks_count,
        timeout_sec: rule.ready_timeout_sec,
    };
    match checker.wait_until_stable(path) {
        Ok(true) => true,
        Ok(false) => {
            fire(
                event_tx,
                AppEvent::Warning {
                    message: format!("Timeout waiting for file to stabilize: {}", path.display()),
                },
            );
            false
        }
        Err(e) => {
            fire(
                event_tx,
                AppEvent::Error {
                    message: format!("Stability check error: {e}"),
                },
            );
            false
        }
    }
}

#[allow(
    missing_debug_implementations,
    reason = "JoinHandle and Sender carry no useful Debug"
)]
pub struct WatcherHandle {
    stop_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl WatcherHandle {
    pub fn stop(mut self) {
        self.shutdown();
    }

    #[allow(
        let_underscore_drop,
        clippy::let_underscore_must_use,
        reason = "watcher is being torn down; result is irrelevant"
    )]
    fn shutdown(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}
