use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestSummary {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

impl TestSummary {
    pub fn new(passed: u32, failed: u32, skipped: u32) -> Self {
        Self { passed, failed, skipped }
    }

    pub fn total(&self) -> u32 {
        self.passed + self.failed + self.skipped
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_new() {
        let s = TestSummary::new(10, 2, 3);
        assert_eq!(s.passed, 10);
        assert_eq!(s.failed, 2);
        assert_eq!(s.skipped, 3);
    }

    #[test]
    fn test_summary_total() {
        let s = TestSummary::new(10, 2, 3);
        assert_eq!(s.total(), 15);
    }

    #[test]
    fn test_summary_all_passed() {
        assert!(TestSummary::new(10, 0, 2).all_passed());
        assert!(!TestSummary::new(10, 1, 0).all_passed());
    }

    #[test]
    fn test_summary_serialize_json() {
        let s = TestSummary::new(42, 0, 2);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"passed\":42"));
        assert!(json.contains("\"failed\":0"));
        assert!(json.contains("\"skipped\":2"));
    }

    #[test]
    fn test_summary_deserialize_json() {
        let json = r#"{"passed":5,"failed":1,"skipped":0}"#;
        let s: TestSummary = serde_json::from_str(json).unwrap();
        assert_eq!(s, TestSummary::new(5, 1, 0));
    }

    #[test]
    fn test_summary_zero() {
        let s = TestSummary::new(0, 0, 0);
        assert_eq!(s.total(), 0);
        assert!(s.all_passed());
    }
}
