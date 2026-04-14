use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommandAction {
    Retry,
    RuntimeError,
    Abort,
    Skip,
    /// LLM parses per-module JUnit test reports and prints a formatted
    /// table. Pipeline flow unaffected — the step is already marked
    /// success via allow_failure.
    TestPrint,
    /// LLM parses the PMD XML report and prints a grouped-by-rule
    /// violations table. Pipeline flow unaffected.
    PmdPrint,
    /// LLM parses the SpotBugs XML report and prints a grouped-by-category
    /// bugs table. Pipeline flow unaffected.
    SpotbugsPrint,
    /// LLM reads the three per-category file lists produced by the git-diff
    /// step and prints them grouped (unstaged / staged / unpushed) to the
    /// terminal. Pipeline flow unaffected.
    GitDiffReport,
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
            (CallbackCommandAction::TestPrint, "\"test_print\""),
            (CallbackCommandAction::PmdPrint, "\"pmd_print\""),
            (CallbackCommandAction::SpotbugsPrint, "\"spotbugs_print\""),
            (CallbackCommandAction::GitDiffReport, "\"git_diff_report\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommandAction = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }
}
