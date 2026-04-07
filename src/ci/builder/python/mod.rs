pub mod mypy;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::builder::{PipelineStrategy, StepDef};
use crate::ci::builder::base::BaseStrategy;

pub struct PythonStrategy;

impl PythonStrategy {
    fn parse_python_test(output: &str) -> Option<String> {
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
        Some(format!("{} passed, {} failed, {} skipped", passed, failed, skipped))
    }
}

impl PipelineStrategy for PythonStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "python-ci".into()
    }

    fn output_report_str(&self, step_name: &str, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        match step_name {
            "test" => Self::parse_python_test(&output)
                .unwrap_or_else(|| BaseStrategy::default_report_str(step_name, success, stdout, stderr)),
            _ => BaseStrategy::default_report_str(step_name, success, stdout, stderr),
        }
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
    use crate::ci::detector::{ProjectInfo, ProjectType};

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
            quality_plugins: vec![],
            subdir: None,
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
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "30 passed, 0 failed, 0 skipped");
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "====== 25 passed, 5 failed in 2.5s ======";
        let strategy = PythonStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "25 passed, 5 failed, 0 skipped");
    }

    #[test]
    fn test_parse_test_output_with_skipped() {
        let output = "====== 20 passed, 2 skipped in 1.0s ======";
        let strategy = PythonStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "20 passed, 0 failed, 2 skipped");
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "collected 0 items / 1 error";
        let strategy = PythonStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "Tests failed");
    }
}
