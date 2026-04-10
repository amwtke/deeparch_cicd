use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommandAction {
    Retry,
    RuntimeError,
    Abort,
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
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommandAction = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }
}
