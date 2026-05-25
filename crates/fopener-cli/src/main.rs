use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fopener", about = "F-Opener \u{2014} Because Created doesn't mean Ready.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch folders and open matching files
    Watch {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
    },
    /// Validate the config file
    ValidateConfig {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
    },
    /// Test a rule against a specific filename
    TestRule {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
        #[arg(long)]
        rule: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Open a specific file using the matching rule
    OpenOnce {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
}

fn main() {
    use fopener_actions::runner::{ActionRunner, ProcessActionRunner};
    use fopener_config::{loader::load_config, validation::validate_config};
    use fopener_core::{
        matching::RuleMatcher,
        types::{FileCandidate, MatchDecision},
    };
    use fopener_watcher::engine::WatcherEngine;
    use std::sync::mpsc;
    use std::sync::Arc;
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Watch { config } => {
            let app_config = match load_config(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {e}");
                    std::process::exit(1);
                }
            };

            let (tx, rx) = mpsc::channel();
            let runner = Arc::new(ProcessActionRunner);
            let active_rules: Vec<_> = app_config.rules.into_iter().filter(|r| r.enabled).collect();

            if active_rules.is_empty() {
                eprintln!("No enabled rules found in config.");
                std::process::exit(1);
            }

            let engine = WatcherEngine::new(active_rules, runner, tx);
            let handle = match engine.run() {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Failed to start watcher: {e}");
                    std::process::exit(1);
                }
            };

            println!("F-Opener watching. Press Ctrl+C to stop.");

            // Print events until channel closes
            for event in rx {
                println!("{}", format_event(&event));
            }

            drop(handle);
        }

        Commands::ValidateConfig { config } => {
            let app_config = match load_config(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {e}");
                    std::process::exit(1);
                }
            };
            match validate_config(&app_config) {
                Ok(errors) if errors.is_empty() => println!("Config is valid."),
                Ok(errors) => {
                    for e in &errors {
                        eprintln!("Error: {e}");
                    }
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Validation failed: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::TestRule { config, rule: rule_id, file } => {
            let app_config = match load_config(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {e}");
                    std::process::exit(1);
                }
            };

            let rule = match app_config.rules.iter().find(|r| r.id == rule_id || r.name == rule_id) {
                Some(r) => r,
                None => {
                    eprintln!("Rule not found: {rule_id}");
                    std::process::exit(1);
                }
            };

            let file_name = file.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Use the rule's folder as the parent if file has no parent
            let test_path = if file.parent().map(|p| p == std::path::Path::new("")).unwrap_or(true) {
                rule.path.join(&file)
            } else {
                file.clone()
            };

            let candidate = FileCandidate {
                path: test_path,
                file_name: file_name.clone(),
                size: None,
            };

            println!("Rule: {}", rule.name);
            println!("File: {}", file_name);
            println!();

            let matcher = match RuleMatcher::new(rule) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Invalid rule: {e}");
                    std::process::exit(1);
                }
            };

            match matcher.decide(rule, &candidate) {
                MatchDecision::Matched => {
                    println!("Wildcard: matched");
                    if rule.regex.is_some() {
                        println!("Regex: matched");
                    }
                    println!("Default ignore patterns: passed");
                    println!();
                    println!("Decision: matched");
                }
                MatchDecision::Ignored { reason } => {
                    println!("Decision: ignored \u{2014} {reason}");
                }
            }
        }

        Commands::OpenOnce { config, file } => {
            let app_config = match load_config(&config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {e}");
                    std::process::exit(1);
                }
            };

            let candidate = FileCandidate::from_path(file.clone());
            let runner = ProcessActionRunner;

            for rule in &app_config.rules {
                if !rule.enabled {
                    continue;
                }
                let matcher = match RuleMatcher::new(rule) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if matches!(matcher.decide(rule, &candidate), MatchDecision::Matched) {
                    println!("Opening {} with rule '{}'...", file.display(), rule.name);
                    match runner.run(rule, &file) {
                        Ok(result) => println!("{}", result.message),
                        Err(e) => eprintln!("Error: {e}"),
                    }
                    return;
                }
            }
            eprintln!("No matching rule found for: {}", file.display());
            std::process::exit(1);
        }
    }
}

fn format_event(event: &fopener_core::types::AppEvent) -> String {
    use fopener_core::types::AppEvent;
    match event {
        AppEvent::RuleStarted { rule_id } => format!("[INFO] Rule started: {rule_id}"),
        AppEvent::RuleStopped { rule_id } => format!("[INFO] Rule stopped: {rule_id}"),
        AppEvent::FileDetected { rule_id, path } => {
            format!("[INFO] [{rule_id}] Detected: {}", path.display())
        }
        AppEvent::FileIgnored { rule_id, path, reason } => {
            format!("[DEBUG] [{rule_id}] Ignored {}: {reason}", path.display())
        }
        AppEvent::FileMatched { rule_id, path } => {
            format!("[INFO] [{rule_id}] Matched: {}", path.display())
        }
        AppEvent::FileReady { rule_id, path } => {
            format!("[INFO] [{rule_id}] File is ready: {}", path.display())
        }
        AppEvent::ActionStarted { rule_id, executable, .. } => {
            format!("[INFO] [{rule_id}] Launching: {}", executable.display())
        }
        AppEvent::ActionCompleted { rule_id, path } => {
            format!("[INFO] [{rule_id}] Opened: {}", path.display())
        }
        AppEvent::ActionFailed { rule_id, path, error } => {
            format!("[ERROR] [{rule_id}] Failed to open {}: {error}", path.display())
        }
        AppEvent::Warning { message } => format!("[WARN] {message}"),
        AppEvent::Error { message } => format!("[ERROR] {message}"),
    }
}
