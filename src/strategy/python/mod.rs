pub mod mypy;

use regex::Regex;
use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;
use crate::strategy::test_parser::TestSummary;

pub struct PythonStrategy;

impl PipelineStrategy for PythonStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "python-ci".into()
    }

    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let passed_re = Regex::new(r"(\d+) passed").unwrap();
        let failed_re = Regex::new(r"(\d+) failed").unwrap();
        let skipped_re = Regex::new(r"(\d+) skipped").unwrap();
        let passed: u32 = passed_re
            .captures(output)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(0);
        let failed: u32 = failed_re
            .captures(output)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(0);
        let skipped: u32 = skipped_re
            .captures(output)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(0);
        if passed == 0 && failed == 0 && skipped == 0 {
            return None;
        }
        Some(TestSummary::new(passed, failed, skipped))
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];
        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }
        steps.push(mypy::step(info));
        steps.push(BaseStrategy::test_step(info));
        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::{ProjectInfo, ProjectType};

    fn make_python_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Python,
            language_version: Some("3.12".into()),
            framework: Some("fastapi".into()),
            image: "python:3.12-slim".into(),
            build_cmd: vec!["pip install -r requirements.txt".into()],
            test_cmd: vec!["pytest".into()],
            lint_cmd: Some(vec!["flake8 .".into()]),
            fmt_cmd: Some(vec!["black --check .".into()]),
            source_paths: vec![".".into()],
            config_files: vec!["pyproject.toml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_python_steps() {
        let info = make_python_info();
        let strategy = PythonStrategy;
        let steps = strategy.steps(&info);
        // build, lint, mypy, test, fmt-check
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "lint");
        assert_eq!(steps[2].name, "mypy");
        assert_eq!(steps[3].name, "test");
        assert_eq!(steps[4].name, "fmt-check");
    }

    #[test]
    fn test_python_pipeline_name() {
        let info = make_python_info();
        let strategy = PythonStrategy;
        assert_eq!(strategy.pipeline_name(&info), "python-ci");
    }

    #[test]
    fn test_parse_test_output_all_pass() {
        let output = "====== 30 passed in 1.23s ======";
        let strategy = PythonStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 30);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "====== 25 passed, 5 failed in 2.5s ======";
        let strategy = PythonStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 25);
        assert_eq!(summary.failed, 5);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_with_skipped() {
        let output = "====== 20 passed, 2 skipped in 1.0s ======";
        let strategy = PythonStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 20);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "collected 0 items / 1 error";
        let strategy = PythonStrategy;
        assert!(strategy.parse_test_output(output).is_none());
    }
}
