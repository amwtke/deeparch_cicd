pub mod mypy_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{BuildStep, FmtStep, LintStep, TestStep};
use crate::ci::pipeline_builder::{PipelineStrategy, StepDef};
use regex::Regex;

pub struct PythonStrategy;

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
    Some(format!(
        "{} passed, {} failed, {} skipped",
        passed, failed, skipped
    ))
}

impl PipelineStrategy for PythonStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "python-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];

        // Build
        steps.push(Box::new(BuildStep::new(info)));

        // Lint (optional)
        if let Some(lint_step) = LintStep::new(info) {
            steps.push(Box::new(lint_step));
        }

        // Mypy
        steps.push(Box::new(mypy_step::MypyStep::new(info)));

        // Test with python parser
        let test_step = TestStep::new(info).with_parser(parse_python_test);
        steps.push(Box::new(test_step));

        // Fmt-check (optional)
        if let Some(fmt_step) = FmtStep::new(info) {
            steps.push(Box::new(fmt_step));
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
            framework: None,
            image: "python:3.12".into(),
            build_cmd: vec!["pip install -e .".into()],
            test_cmd: vec!["pytest".into()],
            lint_cmd: Some(vec!["ruff check .".into()]),
            fmt_cmd: Some(vec!["ruff format --check .".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["pyproject.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_python_info_minimal() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Python,
            language_version: Some("3.12".into()),
            framework: None,
            image: "python:3.12".into(),
            build_cmd: vec!["pip install -e .".into()],
            test_cmd: vec!["pytest".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pyproject.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_python_steps_full() {
        let info = make_python_info();
        let strategy = PythonStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "lint", "mypy", "test", "fmt-check"]);
    }

    #[test]
    fn test_python_steps_minimal() {
        let info = make_python_info_minimal();
        let strategy = PythonStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "mypy", "test"]);
    }

    #[test]
    fn test_python_pipeline_name() {
        let info = make_python_info();
        assert_eq!(PythonStrategy.pipeline_name(&info), "python-ci");
    }

    #[test]
    fn test_parse_python_test_basic() {
        let output = "10 passed, 2 failed, 1 skipped";
        assert_eq!(
            parse_python_test(output).unwrap(),
            "10 passed, 2 failed, 1 skipped"
        );
    }

    #[test]
    fn test_parse_python_test_only_passed() {
        let output = "42 passed";
        assert_eq!(
            parse_python_test(output).unwrap(),
            "42 passed, 0 failed, 0 skipped"
        );
    }

    #[test]
    fn test_parse_python_test_no_match() {
        assert!(parse_python_test("collecting tests...").is_none());
    }
}
