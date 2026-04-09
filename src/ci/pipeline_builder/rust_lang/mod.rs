pub mod clippy_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{BuildStep, FmtStep, TestStep};
use crate::ci::pipeline_builder::{PipelineStrategy, StepConfig, StepDef};
use regex::Regex;

pub struct RustStrategy;

fn parse_rust_test(output: &str) -> Option<String> {
    let re = Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored").unwrap();
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
    Some(format!(
        "{} passed, {} failed, {} ignored",
        total_passed, total_failed, total_ignored
    ))
}

impl PipelineStrategy for RustStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "rust-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];

        // Build
        steps.push(Box::new(BuildStep::new(info)));

        // Clippy (always present)
        steps.push(Box::new(clippy_step::ClippyStep::new(info)));

        // Test with rust parser
        let test_step = TestStep::new(info).with_parser(parse_rust_test);
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

    fn make_rust_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".into(),
            ]),
            fmt_cmd: Some(vec![
                "rustup component add rustfmt 2>/dev/null; cargo fmt -- --check".into(),
            ]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_rust_info_no_fmt() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_rust_steps_with_fmt() {
        let info = make_rust_info();
        let strategy = RustStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "clippy", "test", "fmt-check"]);
    }

    #[test]
    fn test_rust_steps_without_fmt() {
        let info = make_rust_info_no_fmt();
        let strategy = RustStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "clippy", "test"]);
    }

    #[test]
    fn test_clippy_always_present() {
        let info = make_rust_info_no_fmt();
        let strategy = RustStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert!(names.contains(&"clippy".to_string()));
    }

    #[test]
    fn test_rust_pipeline_name() {
        let info = make_rust_info();
        assert_eq!(RustStrategy.pipeline_name(&info), "rust-ci");
    }

    #[test]
    fn test_parse_rust_test_single() {
        let output = "test result: ok. 42 passed; 0 failed; 2 ignored";
        assert_eq!(
            parse_rust_test(output).unwrap(),
            "42 passed, 0 failed, 2 ignored"
        );
    }

    #[test]
    fn test_parse_rust_test_multiple() {
        let output = "\
test result: ok. 10 passed; 0 failed; 1 ignored
test result: FAILED. 5 passed; 2 failed; 0 ignored";
        assert_eq!(
            parse_rust_test(output).unwrap(),
            "15 passed, 2 failed, 1 ignored"
        );
    }

    #[test]
    fn test_parse_rust_test_no_match() {
        assert!(parse_rust_test("Compiling foo v0.1.0").is_none());
    }
}
