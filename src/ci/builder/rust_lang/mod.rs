pub mod clippy;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::builder::{PipelineStrategy, StepDef};
use crate::ci::builder::base::BaseStrategy;

pub struct RustStrategy;

impl RustStrategy {
    fn parse_rust_test(output: &str) -> Option<String> {
        let re = Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored")
            .unwrap();
        // Sum across multiple test binaries
        let mut total_passed: u32 = 0;
        let mut total_failed: u32 = 0;
        let mut total_ignored: u32 = 0;
        let mut found = false;
        for cap in re.captures_iter(output) {
            found = true;
            total_passed += cap[1].parse::<u32>().unwrap_or(0);
            total_failed += cap[2].parse::<u32>().unwrap_or(0);
            total_ignored += cap[3].parse::<u32>().unwrap_or(0);
        }
        if !found {
            return None;
        }
        Some(format!("{} passed, {} failed, {} ignored", total_passed, total_failed, total_ignored))
    }
}

impl PipelineStrategy for RustStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "rust-ci".into()
    }

    fn output_report_str(&self, step_name: &str, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        match step_name {
            "test" => Self::parse_rust_test(&output)
                .unwrap_or_else(|| BaseStrategy::default_report_str(step_name, success, stdout, stderr)),
            _ => BaseStrategy::default_report_str(step_name, success, stdout, stderr),
        }
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
    use crate::ci::detector::{ProjectInfo, ProjectType};

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
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "42 passed, 0 failed, 3 ignored");
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "test result: FAILED. 8 passed; 2 failed; 0 ignored; 0 measured";
        let strategy = RustStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "8 passed, 2 failed, 0 ignored");
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "Compiling pipelight v0.1.0";
        let strategy = RustStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "Tests passed");
    }
}
