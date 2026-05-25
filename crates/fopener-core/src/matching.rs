use std::path::Path;

use globset::{Glob, GlobMatcher};
use regex::Regex;

use crate::error::CoreError;
use crate::types::{FileCandidate, IgnoreReason, MatchDecision, WatchRule};

/// Default ignore patterns — files matching these are silently skipped.
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "*.tmp",
    "*.part",
    "*.crdownload",
    "*.download",
    "~*",
    "*.swp",
];

#[derive(Debug)]
pub struct RuleMatcher {
    glob_matcher: GlobMatcher,
    regex: Option<Regex>,
    ignore_matchers: Vec<(String, GlobMatcher)>,
}

impl RuleMatcher {
    pub fn new(rule: &WatchRule) -> Result<Self, CoreError> {
        let glob = Glob::new(&rule.file_mask).map_err(|e| CoreError::InvalidGlob(e.to_string()))?;
        let glob_matcher = glob.compile_matcher();

        let regex = rule
            .regex
            .as_deref()
            .map(|pattern| Regex::new(pattern).map_err(|e| CoreError::InvalidRegex(e.to_string())))
            .transpose()?;

        let ignore_matchers = DEFAULT_IGNORE_PATTERNS
            .iter()
            .map(|p| {
                // Built-in patterns are static literals; failure here is a
                // bug in the constants, not a runtime condition.
                let g = Glob::new(p).map_err(|e| {
                    CoreError::InvalidGlob(format!("built-in ignore pattern {p:?}: {e}"))
                })?;
                Ok::<_, CoreError>(((*p).to_owned(), g.compile_matcher()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            glob_matcher,
            regex,
            ignore_matchers,
        })
    }

    pub fn decide(&self, rule: &WatchRule, candidate: &FileCandidate) -> MatchDecision {
        if !rule.enabled {
            return MatchDecision::Ignored {
                reason: IgnoreReason::RuleDisabled,
            };
        }

        let in_folder = if rule.include_subdirectories {
            candidate.path.starts_with(&rule.path)
        } else {
            candidate.path.parent() == Some(rule.path.as_path())
        };
        if !in_folder {
            return MatchDecision::Ignored {
                reason: IgnoreReason::NotInFolder,
            };
        }

        if candidate.path.is_dir() {
            return MatchDecision::Ignored {
                reason: IgnoreReason::IsDirectory,
            };
        }

        let file_name = Path::new(&candidate.file_name);

        for (pattern, matcher) in &self.ignore_matchers {
            if matcher.is_match(file_name) {
                return MatchDecision::Ignored {
                    reason: IgnoreReason::DefaultIgnorePattern {
                        pattern: pattern.clone(),
                    },
                };
            }
        }

        if !self.glob_matcher.is_match(file_name) {
            return MatchDecision::Ignored {
                reason: IgnoreReason::MaskNotMatched,
            };
        }

        if let Some(re) = &self.regex {
            if !re.is_match(&candidate.file_name) {
                return MatchDecision::Ignored {
                    reason: IgnoreReason::RegexNotMatched,
                };
            }
        }

        MatchDecision::Matched
    }
}

pub fn validate_glob(pattern: &str) -> Result<(), String> {
    Glob::new(pattern).map(|_| ()).map_err(|e| e.to_string())
}

pub fn validate_regex(pattern: &str) -> Result<(), String> {
    Regex::new(pattern).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    reason = "tests assert invariants; concise unwraps are appropriate"
)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::{ActionTemplate, WatchRule};

    fn make_rule(mask: &str, regex: Option<&str>, path: &str) -> WatchRule {
        WatchRule {
            id: "test".to_owned(),
            name: "Test".to_owned(),
            enabled: true,
            path: PathBuf::from(path),
            include_subdirectories: false,
            file_mask: mask.to_owned(),
            regex: regex.map(str::to_owned),
            action: ActionTemplate {
                executable: PathBuf::from("notepad.exe"),
                arguments: vec!["{file}".to_owned()],
            },
            debounce_ms: 1000,
            wait_until_stable: true,
            stable_check_ms: 300,
            stable_checks_count: 3,
            ready_timeout_sec: 30,
        }
    }

    fn make_candidate(folder: &str, filename: &str) -> FileCandidate {
        let path = PathBuf::from(folder).join(filename);
        FileCandidate {
            path,
            file_name: filename.to_owned(),
            size: Some(1024),
        }
    }

    #[test]
    fn test_glob_match() {
        let rule = make_rule("*.xml", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "export_123.xml");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Matched
        ));
    }

    #[test]
    fn test_glob_no_match() {
        let rule = make_rule("*.xml", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "export_123.csv");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored {
                reason: IgnoreReason::MaskNotMatched
            }
        ));
    }

    #[test]
    fn test_regex_match() {
        let rule = make_rule("*.xml", Some("^export_.*\\.xml$"), "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "export_123.xml");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Matched
        ));
    }

    #[test]
    fn test_regex_no_match() {
        let rule = make_rule("*.xml", Some("^export_.*\\.xml$"), "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "invoice_123.xml");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored {
                reason: IgnoreReason::RegexNotMatched
            }
        ));
    }

    #[test]
    fn test_ignore_tmp() {
        let rule = make_rule("*", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "file.tmp");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored { .. }
        ));
    }

    #[test]
    fn test_ignore_crdownload() {
        let rule = make_rule("*", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "file.crdownload");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored { .. }
        ));
    }

    #[test]
    fn test_ignore_part() {
        let rule = make_rule("*", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "file.part");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored { .. }
        ));
    }

    #[test]
    fn test_rule_disabled() {
        let mut rule = make_rule("*.xml", None, "/watch");
        rule.enabled = false;
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "export.xml");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored {
                reason: IgnoreReason::RuleDisabled
            }
        ));
    }

    #[test]
    fn test_not_in_folder() {
        let rule = make_rule("*.xml", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/other", "export.xml");
        assert!(matches!(
            matcher.decide(&rule, &candidate),
            MatchDecision::Ignored {
                reason: IgnoreReason::NotInFolder
            }
        ));
    }

    #[test]
    fn test_invalid_regex_returns_error() {
        let rule = make_rule("*.xml", Some("[invalid"), "/watch");
        RuleMatcher::new(&rule).unwrap_err();
    }
}
