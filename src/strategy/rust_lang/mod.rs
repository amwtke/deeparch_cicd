pub mod clippy;

use regex::Regex;
use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;
use crate::strategy::test_parser::TestSummary;

pub struct RustStrategy;

impl PipelineStrategy for RustStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "rust-ci".into()
    }

    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let re = Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored")
            .unwrap();
        let cap = re.captures(output)?;
        let passed: u32 = cap[1].parse().unwrap_or(0);
        let failed: u32 = cap[2].parse().unwrap_or(0);
        let skipped: u32 = cap[3].parse().unwrap_or(0);
        Some(TestSummary::new(passed, failed, skipped))
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![
            BaseStrategy::build_step(info),
            clippy::step(info),
            BaseStrategy::test_step(info),
        ];
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

    fn make_rust_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_rust_steps() {
        let info = make_rust_info();
        let strategy = RustStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "clippy");
        assert_eq!(steps[2].name, "test");
        assert_eq!(steps[3].name, "fmt-check");
    }

    #[test]
    fn test_rust_clippy_always_present() {
        // clippy is always present, even without lint_cmd
        let mut info = make_rust_info();
        info.lint_cmd = None;
        let strategy = RustStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"clippy"));
    }

    #[test]
    fn test_rust_pipeline_name() {
        let info = make_rust_info();
        let strategy = RustStrategy;
        assert_eq!(strategy.pipeline_name(&info), "rust-ci");
    }

    #[test]
    fn test_parse_test_output_all_pass() {
        let output = "test result: ok. 42 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out";
        let strategy = RustStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 42);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 3);
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "test result: FAILED. 8 passed; 2 failed; 0 ignored; 0 measured";
        let strategy = RustStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 8);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "Compiling pipelight v0.1.0";
        let strategy = RustStrategy;
        assert!(strategy.parse_test_output(output).is_none());
    }
}
