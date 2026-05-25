use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{mpsc, Arc};

use clap::{Parser, Subcommand};
use fopener_actions::runner::{ActionRunner, ProcessActionRunner};
use fopener_config::loader::load_config;
use fopener_config::validation::validate_config;
use fopener_core::matching::RuleMatcher;
use fopener_core::types::{AppEvent, FileCandidate, MatchDecision};
use fopener_watcher::engine::WatcherEngine;
use tracing::Level;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "fopener",
    about = "F-Opener \u{2014} Because Created doesn't mean Ready."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Watch folders and open matching files.
    Watch {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
    },
    /// Validate the config file.
    ValidateConfig {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
    },
    /// Test a rule against a specific filename.
    TestRule {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
        #[arg(long)]
        rule: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Open a specific file using the matching rule.
    OpenOnce {
        #[arg(long, default_value = "config.json")]
        config: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Watch { config } => cmd_watch(&config),
        Commands::ValidateConfig { config } => cmd_validate(&config),
        Commands::TestRule { config, rule, file } => cmd_test_rule(&config, &rule, &file),
        Commands::OpenOnce { config, file } => cmd_open_once(&config, &file),
    }
}

fn cmd_watch(config_path: &Path) -> ExitCode {
    let app_config = match load_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            return ExitCode::FAILURE;
        }
    };

    let active_rules: Vec<_> = app_config.rules.into_iter().filter(|r| r.enabled).collect();
    if active_rules.is_empty() {
        eprintln!("No enabled rules found in config.");
        return ExitCode::FAILURE;
    }

    let (tx, rx) = mpsc::channel();
    let runner: Arc<dyn ActionRunner> = Arc::new(ProcessActionRunner);
    let engine = WatcherEngine::new(active_rules, runner, tx);
    let handle = match engine.run() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to start watcher: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("F-Opener watching. Press Ctrl+C to stop.");
    for event in rx {
        println!("{}", format_event(&event));
    }
    drop(handle);
    ExitCode::SUCCESS
}

fn cmd_validate(config_path: &Path) -> ExitCode {
    let app_config = match load_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            return ExitCode::FAILURE;
        }
    };
    match validate_config(&app_config) {
        Ok(errors) if errors.is_empty() => {
            println!("Config is valid.");
            ExitCode::SUCCESS
        }
        Ok(errors) => {
            for e in &errors {
                eprintln!("Error: {e}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("Validation failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_test_rule(config_path: &Path, rule_id: &str, file: &Path) -> ExitCode {
    let app_config = match load_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            return ExitCode::FAILURE;
        }
    };

    let Some(rule) = app_config
        .rules
        .iter()
        .find(|r| r.id == rule_id || r.name == rule_id)
    else {
        eprintln!("Rule not found: {rule_id}");
        return ExitCode::FAILURE;
    };

    let file_name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());

    let test_path = match file.parent() {
        Some(p) if p != Path::new("") => file.to_path_buf(),
        _ => rule.path.join(file),
    };

    let candidate = FileCandidate {
        path: test_path,
        file_name: file_name.clone(),
        size: None,
    };

    println!("Rule: {}", rule.name);
    println!("File: {file_name}");
    println!();

    let matcher = match RuleMatcher::new(rule) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Invalid rule: {e}");
            return ExitCode::FAILURE;
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
    ExitCode::SUCCESS
}

fn cmd_open_once(config_path: &Path, file: &Path) -> ExitCode {
    let app_config = match load_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            return ExitCode::FAILURE;
        }
    };

    let candidate = FileCandidate::from_path(file.to_path_buf());
    let runner = ProcessActionRunner;

    for rule in &app_config.rules {
        if !rule.enabled {
            continue;
        }
        let Ok(matcher) = RuleMatcher::new(rule) else {
            continue;
        };
        if matches!(matcher.decide(rule, &candidate), MatchDecision::Matched) {
            println!("Opening {} with rule '{}'...", file.display(), rule.name);
            match runner.run(rule, file) {
                Ok(result) => println!("{}", result.message),
                Err(e) => eprintln!("Error: {e}"),
            }
            return ExitCode::SUCCESS;
        }
    }
    eprintln!("No matching rule found for: {}", file.display());
    ExitCode::FAILURE
}

fn format_event(event: &AppEvent) -> String {
    match event {
        AppEvent::RuleStarted { rule_id } => format!("[INFO] Rule started: {rule_id}"),
        AppEvent::RuleStopped { rule_id } => format!("[INFO] Rule stopped: {rule_id}"),
        AppEvent::FileDetected { rule_id, path } => {
            format!("[INFO] [{rule_id}] Detected: {}", path.display())
        }
        AppEvent::FileIgnored {
            rule_id,
            path,
            reason,
        } => {
            format!("[DEBUG] [{rule_id}] Ignored {}: {reason}", path.display())
        }
        AppEvent::FileMatched { rule_id, path } => {
            format!("[INFO] [{rule_id}] Matched: {}", path.display())
        }
        AppEvent::FileReady { rule_id, path } => {
            format!("[INFO] [{rule_id}] File is ready: {}", path.display())
        }
        AppEvent::ActionStarted {
            rule_id,
            executable,
            ..
        } => {
            format!("[INFO] [{rule_id}] Launching: {}", executable.display())
        }
        AppEvent::ActionCompleted { rule_id, path } => {
            format!("[INFO] [{rule_id}] Opened: {}", path.display())
        }
        AppEvent::ActionFailed {
            rule_id,
            path,
            error,
        } => {
            format!(
                "[ERROR] [{rule_id}] Failed to open {}: {error}",
                path.display()
            )
        }
        AppEvent::Warning { message } => format!("[WARN] {message}"),
        AppEvent::Error { message } => format!("[ERROR] {message}"),
    }
}
