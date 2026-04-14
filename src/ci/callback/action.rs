use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommandAction {
    Retry,
    RuntimeError,
    Abort,
    Skip,
    /// LLM consumes the callback info and produces a formatted report.
    /// Pipeline flow is unaffected — the step keeps its existing status
    /// (success via allow_failure, or failed). Used for post-run analysis
    /// tasks like formatting a per-module test table.
    Print,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip() {
        for (variant, expected_str) in [
            (CallbackCommandAction::Retry, "\"retry\""),
            (CallbackCommandAction::RuntimeError, "\"runtime_error\""),
            (CallbackCommandAction::Abort, "\"abort\""),
            (CallbackCommandAction::Skip, "\"skip\""),
            (CallbackCommandAction::Print, "\"print\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommandAction = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }
}
