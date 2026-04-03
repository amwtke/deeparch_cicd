pub mod vet;

use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;

pub struct GoStrategy;

impl PipelineStrategy for GoStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "go-ci".into()
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
}
