use crate::types::{FileCandidate, IgnoreReason, MatchDecision, WatchRule};
use globset::{Glob, GlobMatcher};
use regex::Regex;
use std::path::Path;

/// Default ignore patterns — files matching these are silently skipped
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "*.tmp",
    "*.part",
    "*.crdownload",
    "*.download",
    "~*",
    "*.swp",
];

pub struct RuleMatcher {
    #[allow(dead_code)]
    rule_id: String,
    glob_matcher: GlobMatcher,
    regex: Option<Regex>,
    ignore_matchers: Vec<(String, GlobMatcher)>,
}

impl RuleMatcher {
    pub fn new(rule: &WatchRule) -> Result<Self, crate::error::CoreError> {
        let glob = Glob::new(&rule.file_mask)
            .map_err(|e| crate::error::CoreError::InvalidGlob(e.to_string()))?;
        let glob_matcher = glob.compile_matcher();

        let regex = if let Some(ref pattern) = rule.regex {
            Some(
                Regex::new(pattern)
                    .map_err(|e| crate::error::CoreError::InvalidRegex(e.to_string()))?,
            )
        } else {
            None
        };

        let ignore_matchers = DEFAULT_IGNORE_PATTERNS
            .iter()
            .map(|p| {
                let g = Glob::new(p).expect("built-in ignore pattern is valid");
                (p.to_string(), g.compile_matcher())
            })
            .collect();

        Ok(Self {
            rule_id: rule.id.clone(),
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

        // Check path containment
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

        // Must be a file (skip directories)
        if candidate.path.is_dir() {
            return MatchDecision::Ignored {
                reason: IgnoreReason::IsDirectory,
            };
        }

        let file_name = Path::new(&candidate.file_name);

        // Check default ignore patterns first
        for (pattern, matcher) in &self.ignore_matchers {
            if matcher.is_match(file_name) {
                return MatchDecision::Ignored {
                    reason: IgnoreReason::DefaultIgnorePattern {
                        pattern: pattern.clone(),
                    },
                };
            }
        }

        // Check glob mask
        if !self.glob_matcher.is_match(file_name) {
            return MatchDecision::Ignored {
                reason: IgnoreReason::MaskNotMatched,
            };
        }

        // Check optional regex
        if let Some(ref re) = self.regex {
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
mod tests {
    use super::*;
    use crate::types::{ActionTemplate, WatchRule};
    use std::path::PathBuf;

    fn make_rule(mask: &str, regex: Option<&str>, path: &str) -> WatchRule {
        WatchRule {
            id: "test".to_string(),
            name: "Test".to_string(),
            enabled: true,
            path: PathBuf::from(path),
            include_subdirectories: false,
            file_mask: mask.to_string(),
            regex: regex.map(|s| s.to_string()),
            action: ActionTemplate {
                executable: PathBuf::from("notepad.exe"),
                arguments: vec!["{file}".to_string()],
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
            file_name: filename.to_string(),
            size: Some(1024),
        }
    }

    #[test]
    fn test_glob_match() {
        let rule = make_rule("*.xml", None, "/watch");
        let matcher = RuleMatcher::new(&rule).unwrap();
        let candidate = make_candidate("/watch", "export_123.xml");
        assert!(matches!(matcher.decide(&rule, &candidate), MatchDecision::Matched));
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
        assert!(matches!(matcher.decide(&rule, &candidate), MatchDecision::Matched));
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
        assert!(RuleMatcher::new(&rule).is_err());
    }
}
