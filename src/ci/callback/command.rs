use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::action::CallbackCommandAction;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommand {
    RuntimeError,
    Abort,
    AutoFix,
    AutoGenPmdRuleset,
    FailAndSkip,
    GitFail,
    Ping,
    TestPrintCommand,
    PmdPrintCommand,
    SpotbugsPrintCommand,
    GitDiffCommand,
}

pub struct CallbackCommandDef {
    pub action: CallbackCommandAction,
    #[allow(dead_code)]
    pub description: String,
}

pub struct CallbackCommandRegistry {
    commands: HashMap<CallbackCommand, CallbackCommandDef>,
}

impl CallbackCommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        registry.register(
            CallbackCommand::RuntimeError,
            CallbackCommandDef {
                action: CallbackCommandAction::RuntimeError,
                description: "Runtime error in tool itself. Pipeline terminates.".into(),
            },
        );
        registry.register(
            CallbackCommand::Abort,
            CallbackCommandDef {
                action: CallbackCommandAction::Abort,
                description: "Tool detected serious code problem. Pipeline terminates.".into(),
            },
        );
        registry.register(
            CallbackCommand::AutoFix,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description: "LLM reads stderr + context_paths, fixes source code, then retries."
                    .into(),
            },
        );
        registry.register(
            CallbackCommand::AutoGenPmdRuleset,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description:
                    "LLM searches project for PMD ruleset or coding guidelines, generates pmd-ruleset.xml, then retries."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::FailAndSkip,
            CallbackCommandDef {
                action: CallbackCommandAction::Skip,
                description:
                    "Step cannot run because prerequisites are missing. Step is marked as skipped and pipeline continues."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::GitFail,
            CallbackCommandDef {
                action: CallbackCommandAction::Skip,
                description:
                    "Git operation failed (network, auth, merge conflict). Step is marked as skipped and pipeline continues."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::Ping,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description:
                    "Ping-pong communication test. LLM prints 'pong' and retries the step.".into(),
            },
        );
        registry.register(
            CallbackCommand::TestPrintCommand,
            CallbackCommandDef {
                action: CallbackCommandAction::TestPrint,
                description:
                    "Test run finished with failures (report-only). LLM parses per-module JUnit XML reports and prints a formatted summary table. Pipeline continues."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::PmdPrintCommand,
            CallbackCommandDef {
                action: CallbackCommandAction::PmdPrint,
                description:
                    "PMD scan found violations (report-only). LLM parses the PMD XML report and prints a grouped-by-rule violations table. Pipeline continues."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::SpotbugsPrintCommand,
            CallbackCommandDef {
                action: CallbackCommandAction::SpotbugsPrint,
                description:
                    "SpotBugs scan found bugs (report-only). LLM parses the SpotBugs XML report and prints a grouped-by-category bugs table. Pipeline continues."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::GitDiffCommand,
            CallbackCommandDef {
                action: CallbackCommandAction::GitDiffReport,
                description:
                    "git-diff found uncommitted or unpushed changes (report-only). LLM reads the three per-category file lists (unstaged / staged / unpushed) from pipelight-misc/git-diff-report/ and prints them grouped to the terminal. Pipeline continues."
                        .into(),
            },
        );
        registry
    }

    pub fn register(&mut self, command: CallbackCommand, def: CallbackCommandDef) {
        self.commands.insert(command, def);
    }

    #[allow(dead_code)]
    pub fn get(&self, command: &CallbackCommand) -> Option<&CallbackCommandDef> {
        self.commands.get(command)
    }

    pub fn action_for(&self, command: &CallbackCommand) -> CallbackCommandAction {
        self.commands
            .get(command)
            .map(|def| def.action.clone())
            .unwrap_or(CallbackCommandAction::RuntimeError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip_all_variants() {
        for (variant, expected_str) in [
            (CallbackCommand::RuntimeError, "\"runtime_error\""),
            (CallbackCommand::Abort, "\"abort\""),
            (CallbackCommand::AutoFix, "\"auto_fix\""),
            (
                CallbackCommand::AutoGenPmdRuleset,
                "\"auto_gen_pmd_ruleset\"",
            ),
            (CallbackCommand::FailAndSkip, "\"fail_and_skip\""),
            (CallbackCommand::GitFail, "\"git_fail\""),
            (CallbackCommand::Ping, "\"ping\""),
            (CallbackCommand::TestPrintCommand, "\"test_print_command\""),
            (CallbackCommand::PmdPrintCommand, "\"pmd_print_command\""),
            (
                CallbackCommand::SpotbugsPrintCommand,
                "\"spotbugs_print_command\"",
            ),
            (CallbackCommand::GitDiffCommand, "\"git_diff_command\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommand = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn test_registry_built_in_commands() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::RuntimeError),
            CallbackCommandAction::RuntimeError
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::Abort),
            CallbackCommandAction::Abort
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::AutoFix),
            CallbackCommandAction::Retry
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::AutoGenPmdRuleset),
            CallbackCommandAction::Retry
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::FailAndSkip),
            CallbackCommandAction::Skip
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::GitFail),
            CallbackCommandAction::Skip
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::Ping),
            CallbackCommandAction::Retry
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::TestPrintCommand),
            CallbackCommandAction::TestPrint
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::PmdPrintCommand),
            CallbackCommandAction::PmdPrint
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::SpotbugsPrintCommand),
            CallbackCommandAction::SpotbugsPrint
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::GitDiffCommand),
            CallbackCommandAction::GitDiffReport
        );
    }

    #[test]
    fn test_registry_get_description() {
        let registry = CallbackCommandRegistry::new();
        let def = registry.get(&CallbackCommand::AutoFix).unwrap();
        assert!(def.description.contains("fixes source code"));
    }

    #[test]
    fn test_registry_git_fail_description() {
        let registry = CallbackCommandRegistry::new();
        let def = registry.get(&CallbackCommand::GitFail).unwrap();
        assert!(def.description.contains("Git operation failed"));
    }

    #[test]
    fn test_registry_all_variants_registered() {
        let registry = CallbackCommandRegistry::new();
        assert!(registry.get(&CallbackCommand::RuntimeError).is_some());
        assert!(registry.get(&CallbackCommand::Abort).is_some());
        assert!(registry.get(&CallbackCommand::AutoFix).is_some());
        assert!(registry.get(&CallbackCommand::AutoGenPmdRuleset).is_some());
        assert!(registry.get(&CallbackCommand::FailAndSkip).is_some());
        assert!(registry.get(&CallbackCommand::GitFail).is_some());
        assert!(registry.get(&CallbackCommand::Ping).is_some());
        assert!(registry.get(&CallbackCommand::TestPrintCommand).is_some());
        assert!(registry.get(&CallbackCommand::PmdPrintCommand).is_some());
        assert!(registry
            .get(&CallbackCommand::SpotbugsPrintCommand)
            .is_some());
        assert!(registry.get(&CallbackCommand::GitDiffCommand).is_some());
    }

    #[test]
    fn test_registry_ping_description() {
        let registry = CallbackCommandRegistry::new();
        let def = registry.get(&CallbackCommand::Ping).unwrap();
        assert!(def.description.contains("Ping-pong"));
    }
}
