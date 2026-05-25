use std::fmt;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type RuleId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchRule {
    pub id: RuleId,
    pub name: String,
    pub enabled: bool,
    pub path: PathBuf,
    pub include_subdirectories: bool,
    pub file_mask: String,
    pub regex: Option<String>,
    pub action: ActionTemplate,
    pub debounce_ms: u64,
    pub wait_until_stable: bool,
    pub stable_check_ms: u64,
    pub stable_checks_count: u32,
    pub ready_timeout_sec: u64,
}

impl WatchRule {
    pub fn new_default(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            enabled: true,
            path: PathBuf::from("."),
            include_subdirectories: false,
            file_mask: "*".to_owned(),
            regex: None,
            action: ActionTemplate {
                executable: PathBuf::new(),
                arguments: vec!["{file}".to_owned()],
            },
            debounce_ms: 1000,
            wait_until_stable: true,
            stable_check_ms: 300,
            stable_checks_count: 3,
            ready_timeout_sec: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTemplate {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FileCandidate {
    pub path: PathBuf,
    pub file_name: String,
    pub size: Option<u64>,
}

impl FileCandidate {
    pub fn from_path(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = fs::metadata(&path).ok().map(|m| m.len());
        Self {
            path,
            file_name,
            size,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MatchDecision {
    Matched,
    Ignored { reason: IgnoreReason },
}

#[derive(Debug, Clone)]
pub enum IgnoreReason {
    RuleDisabled,
    NotInFolder,
    IsDirectory,
    MaskNotMatched,
    RegexNotMatched,
    DefaultIgnorePattern { pattern: String },
}

impl fmt::Display for IgnoreReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuleDisabled => write!(f, "rule is disabled"),
            Self::NotInFolder => write!(f, "not in watched folder"),
            Self::IsDirectory => write!(f, "is a directory"),
            Self::MaskNotMatched => write!(f, "file mask not matched"),
            Self::RegexNotMatched => write!(f, "regex not matched"),
            Self::DefaultIgnorePattern { pattern } => {
                write!(f, "matches ignore pattern: {pattern}")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    RuleStarted {
        rule_id: RuleId,
    },
    RuleStopped {
        rule_id: RuleId,
    },
    FileDetected {
        rule_id: RuleId,
        path: PathBuf,
    },
    FileIgnored {
        rule_id: RuleId,
        path: PathBuf,
        reason: String,
    },
    FileMatched {
        rule_id: RuleId,
        path: PathBuf,
    },
    FileReady {
        rule_id: RuleId,
        path: PathBuf,
    },
    ActionStarted {
        rule_id: RuleId,
        executable: PathBuf,
        arguments: Vec<String>,
    },
    ActionCompleted {
        rule_id: RuleId,
        path: PathBuf,
    },
    ActionFailed {
        rule_id: RuleId,
        path: PathBuf,
        error: String,
    },
    Warning {
        message: String,
    },
    Error {
        message: String,
    },
}
