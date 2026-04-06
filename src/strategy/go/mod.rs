pub mod vet;

use regex::Regex;
use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;
use crate::strategy::test_parser::TestSummary;

pub struct GoStrategy;

impl PipelineStrategy for GoStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "go-ci".into()
    }

    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let ok_re = Regex::new(r"(?m)^ok\s+").unwrap();
        let fail_re = Regex::new(r"(?m)^FAIL\s+").unwrap();
        let passed = ok_re.find_iter(output).count() as u32;
        let failed = fail_re.find_iter(output).count() as u32;
        if passed == 0 && failed == 0 {
            return None;
        }
        Some(TestSummary::new(passed, failed, 0))
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![
            BaseStrategy::build_step(info),
            vet::step(info),
        ];
        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }
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

    fn make_go_info_full() -> ProjectInfo {
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

    fn make_go_info_no_lint() -> ProjectInfo {
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
    fn test_go_steps() {
        let info = make_go_info_full();
        let strategy = GoStrategy;
        let steps = strategy.steps(&info);
        // build, vet, lint, test, fmt-check
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "vet");
        assert_eq!(steps[2].name, "lint");
        assert_eq!(steps[3].name, "test");
        assert_eq!(steps[4].name, "fmt-check");
    }

    #[test]
    fn test_go_steps_no_lint() {
        let info = make_go_info_no_lint();
        let strategy = GoStrategy;
        let steps = strategy.steps(&info);
        // build, vet, test
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "vet");
        assert_eq!(steps[2].name, "test");
    }

    #[test]
    fn test_go_pipeline_name() {
        let info = make_go_info_no_lint();
        let strategy = GoStrategy;
        assert_eq!(strategy.pipeline_name(&info), "go-ci");
    }

    #[test]
    fn test_parse_test_output_mixed() {
        let output = "\
ok  \tgithub.com/example/foo\t0.012s
ok  \tgithub.com/example/bar\t0.005s
FAIL\tgithub.com/example/baz\t0.034s";
        let strategy = GoStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_all_ok() {
        let output = "\
ok  \tgithub.com/example/a\t0.001s
ok  \tgithub.com/example/b\t0.002s";
        let strategy = GoStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "go: downloading github.com/example/dep v1.0.0";
        let strategy = GoStrategy;
        assert!(strategy.parse_test_output(output).is_none());
    }
}
