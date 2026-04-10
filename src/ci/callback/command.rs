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
    }

    #[test]
    fn test_registry_get_description() {
        let registry = CallbackCommandRegistry::new();
        let def = registry.get(&CallbackCommand::AutoFix).unwrap();
        assert!(def.description.contains("fixes source code"));
    }

    #[test]
    fn test_registry_all_variants_registered() {
        let registry = CallbackCommandRegistry::new();
        assert!(registry.get(&CallbackCommand::RuntimeError).is_some());
        assert!(registry.get(&CallbackCommand::Abort).is_some());
        assert!(registry.get(&CallbackCommand::AutoFix).is_some());
        assert!(registry.get(&CallbackCommand::AutoGenPmdRuleset).is_some());
        assert!(registry.get(&CallbackCommand::FailAndSkip).is_some());
    }
}
