use std::collections::HashMap;

use super::command::CallbackCommand;

#[allow(dead_code)]
pub struct ExceptionEntry {
    pub command: CallbackCommand,
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}

#[allow(dead_code)]
pub struct ExceptionMapping {
    entries: HashMap<String, ExceptionEntry>,
    default_command: CallbackCommand,
}

#[allow(dead_code)]
pub struct ResolvedFailure {
    pub exception_key: String,
    pub command: CallbackCommand,
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}

impl ExceptionMapping {
    #[allow(dead_code)]
    pub fn new(default_command: CallbackCommand) -> Self {
        Self {
            entries: HashMap::new(),
            default_command,
        }
    }

    #[allow(dead_code)]
    pub fn add(mut self, key: &str, entry: ExceptionEntry) -> Self {
        self.entries.insert(key.into(), entry);
        self
    }

    /// Resolution chain:
    /// 1. Parse stderr for PIPELIGHT_EXCEPTION:<key> marker
    /// 2. Call match_fn (StepDef::match_exception) for Rust-side analysis
    /// 3. Fallback to default_command
    #[allow(dead_code, clippy::type_complexity)]
    pub fn resolve(
        &self,
        exit_code: i64,
        stdout: &str,
        stderr: &str,
        match_fn: Option<&dyn Fn(i64, &str, &str) -> Option<String>>,
    ) -> ResolvedFailure {
        let exception_key = Self::parse_stderr_marker(stderr)
            .or_else(|| match_fn.and_then(|f| f(exit_code, stdout, stderr)));

        match exception_key {
            Some(key) if self.entries.contains_key(&key) => {
                let entry = &self.entries[&key];
                ResolvedFailure {
                    exception_key: key,
                    command: entry.command.clone(),
                    max_retries: entry.max_retries,
                    context_paths: entry.context_paths.clone(),
                }
            }
            _ => {
                let key = exception_key.unwrap_or_else(|| "unrecognized".into());
                ResolvedFailure {
                    exception_key: key,
                    command: self.default_command.clone(),
                    max_retries: 0,
                    context_paths: vec![],
                }
            }
        }
    }

    #[allow(dead_code)]
    fn parse_stderr_marker(stderr: &str) -> Option<String> {
        for line in stderr.lines() {
            if let Some(rest) = line.strip_prefix("PIPELIGHT_EXCEPTION:") {
                let key = rest.split_whitespace().next().unwrap_or(rest).trim();
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_mapping() -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "ruleset_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 2,
                    context_paths: vec!["src/".into()],
                },
            )
            .add(
                "compile_error",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 3,
                    context_paths: vec!["src/".into(), "Cargo.toml".into()],
                },
            )
    }

    #[test]
    fn test_resolve_stderr_marker_priority() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "some output\nPIPELIGHT_EXCEPTION:ruleset_not_found details here\n",
            None,
        );
        assert_eq!(resolved.exception_key, "ruleset_not_found");
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(resolved.max_retries, 2);
        assert_eq!(resolved.context_paths, vec!["src/"]);
    }

    #[test]
    fn test_resolve_match_fn_fallback() {
        let mapping = test_mapping();
        let match_fn = |_ec: i64, _out: &str, err: &str| -> Option<String> {
            if err.contains("cannot find value") {
                Some("compile_error".into())
            } else {
                None
            }
        };
        let resolved = mapping.resolve(1, "", "error: cannot find value", Some(&match_fn));
        assert_eq!(resolved.exception_key, "compile_error");
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 3);
    }

    #[test]
    fn test_resolve_stderr_marker_beats_match_fn() {
        let mapping = test_mapping();
        let match_fn = |_ec: i64, _out: &str, _err: &str| -> Option<String> {
            Some("compile_error".into())
        };
        let resolved = mapping.resolve(
            1,
            "",
            "PIPELIGHT_EXCEPTION:ruleset_not_found\nerror: cannot find value",
            Some(&match_fn),
        );
        assert_eq!(resolved.exception_key, "ruleset_not_found");
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
    }

    #[test]
    fn test_resolve_default_fallback() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(1, "", "some unknown error", None);
        assert_eq!(resolved.exception_key, "unrecognized");
        assert_eq!(resolved.command, CallbackCommand::RuntimeError);
        assert_eq!(resolved.max_retries, 0);
        assert!(resolved.context_paths.is_empty());
    }

    #[test]
    fn test_resolve_unmapped_exception_key() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "PIPELIGHT_EXCEPTION:totally_unknown_key\n",
            None,
        );
        assert_eq!(resolved.exception_key, "totally_unknown_key");
        assert_eq!(resolved.command, CallbackCommand::RuntimeError);
        assert_eq!(resolved.max_retries, 0);
    }

    #[test]
    fn test_parse_stderr_marker_first_match() {
        let stderr = "line1\nPIPELIGHT_EXCEPTION:first_key\nPIPELIGHT_EXCEPTION:second_key\n";
        let key = ExceptionMapping::parse_stderr_marker(stderr);
        assert_eq!(key, Some("first_key".into()));
    }

    #[test]
    fn test_parse_stderr_marker_empty_key_ignored() {
        let stderr = "PIPELIGHT_EXCEPTION: \nPIPELIGHT_EXCEPTION:valid_key\n";
        let key = ExceptionMapping::parse_stderr_marker(stderr);
        assert_eq!(key, Some("valid_key".into()));
    }

    #[test]
    fn test_parse_stderr_marker_none_when_absent() {
        let key = ExceptionMapping::parse_stderr_marker("just some error output\n");
        assert!(key.is_none());
    }
}
