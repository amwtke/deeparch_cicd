pub mod vet_step;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{PipelineStrategy, StepConfig, StepDef};
use crate::ci::pipeline_builder::base::{BuildStep, TestStep, LintStep, FmtStep};

pub struct GoStrategy;

fn parse_go_test(output: &str) -> Option<String> {
    let ok_re = Regex::new(r"(?m)^ok\s+").unwrap();
    let fail_re = Regex::new(r"(?m)^FAIL\s+").unwrap();
    let passed = ok_re.find_iter(output).count() as u32;
    let failed = fail_re.find_iter(output).count() as u32;
    if passed == 0 && failed == 0 { return None; }
    Some(format!("{} passed, {} failed", passed, failed))
}

impl PipelineStrategy for GoStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "go-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];

        // Build
        steps.push(Box::new(BuildStep::new(info)));

        // Vet (always present)
        steps.push(Box::new(vet_step::VetStep::new(info)));

        // Lint (optional)
        if let Some(lint_step) = LintStep::new(info) {
            steps.push(Box::new(lint_step));
        }

        // Test with go parser
        let test_step = TestStep::new(info).with_parser(parse_go_test);
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

    fn make_go_info_with_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: Some(vec!["golangci-lint run".into()]),
            fmt_cmd: Some(vec!["gofmt -l .".into()]),
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_go_info_without_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_go_steps_with_lint() {
        let info = make_go_info_with_lint();
        let strategy = GoStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "vet", "lint", "test", "fmt-check"]);
    }

    #[test]
    fn test_go_steps_without_lint() {
        let info = make_go_info_without_lint();
        let strategy = GoStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "vet", "test"]);
    }

    #[test]
    fn test_go_pipeline_name() {
        let info = make_go_info_with_lint();
        assert_eq!(GoStrategy.pipeline_name(&info), "go-ci");
    }

    #[test]
    fn test_parse_go_test_basic() {
        let output = "ok  \tgithub.com/foo/bar\t0.5s\nok  \tgithub.com/foo/baz\t1.2s";
        assert_eq!(parse_go_test(output).unwrap(), "2 passed, 0 failed");
    }

    #[test]
    fn test_parse_go_test_with_failures() {
        let output = "ok  \tgithub.com/foo/bar\t0.5s\nFAIL\tgithub.com/foo/baz\t1.2s";
        assert_eq!(parse_go_test(output).unwrap(), "1 passed, 1 failed");
    }

    #[test]
    fn test_parse_go_test_no_match() {
        assert!(parse_go_test("go: downloading module").is_none());
    }
}
