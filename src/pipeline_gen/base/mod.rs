use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::pipeline_gen::{PipelineStrategy, StepDef};

pub struct BaseStrategy;

impl BaseStrategy {
    pub fn build_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "build".into(),
            image: info.image.clone(),
            commands: info.build_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 3,
                context_paths: [&info.source_paths[..], &info.config_files[..]].concat(),
            }),
            ..Default::default()
        }
    }

    pub fn test_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "test".into(),
            image: info.image.clone(),
            commands: info.test_cmd.clone(),
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::Notify,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    pub fn lint_step(info: &ProjectInfo) -> Option<StepDef> {
        info.lint_cmd.as_ref().map(|cmd| StepDef {
            name: "lint".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }

    pub fn fmt_step(info: &ProjectInfo) -> Option<StepDef> {
        info.fmt_cmd.as_ref().map(|cmd| StepDef {
            name: "fmt-check".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 1,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }
}

pub struct BaseOnlyStrategy;

impl PipelineStrategy for BaseOnlyStrategy {
    fn pipeline_name(&self, info: &ProjectInfo) -> String {
        format!(
            "{}-ci",
            format!("{}", info.project_type)
                .to_lowercase()
                .replace('/', "-")
        )
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];
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
    use crate::pipeline::Strategy;

    fn make_info_full() -> ProjectInfo {
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

    fn make_info_minimal() -> ProjectInfo {
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
    fn test_build_step_defaults() {
        let info = make_info_full();
        let step = BaseStrategy::build_step(&info);
        assert_eq!(step.name, "build");
        assert_eq!(step.image, "rust:1.78-slim");
        assert_eq!(step.commands, vec!["cargo build"]);
        assert!(step.depends_on.is_empty());
        assert_eq!(step.workdir, "/workspace");
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::AutoFix);
        assert_eq!(on_failure.max_retries, 3);
        // context_paths includes source_paths + config_files
        assert!(on_failure.context_paths.contains(&"src/".to_string()));
        assert!(on_failure.context_paths.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_test_step_depends_on_build() {
        let info = make_info_full();
        let step = BaseStrategy::test_step(&info);
        assert_eq!(step.name, "test");
        assert_eq!(step.depends_on, vec!["build"]);
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::Notify);
        assert_eq!(on_failure.max_retries, 0);
    }

    #[test]
    fn test_lint_step_returns_none_when_no_lint_cmd() {
        let info = make_info_minimal();
        assert!(BaseStrategy::lint_step(&info).is_none());
    }

    #[test]
    fn test_lint_step_returns_some() {
        let info = make_info_full();
        let step = BaseStrategy::lint_step(&info).unwrap();
        assert_eq!(step.name, "lint");
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::AutoFix);
        assert_eq!(on_failure.max_retries, 2);
    }

    #[test]
    fn test_fmt_step_returns_none_when_no_fmt_cmd() {
        let info = make_info_minimal();
        assert!(BaseStrategy::fmt_step(&info).is_none());
    }

    #[test]
    fn test_base_only_strategy_full_steps() {
        let info = make_info_full();
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "lint");
        assert_eq!(steps[2].name, "test");
        assert_eq!(steps[3].name, "fmt-check");
    }

    #[test]
    fn test_base_only_strategy_minimal_steps() {
        let info = make_info_minimal();
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
    }

    #[test]
    fn test_base_only_pipeline_name() {
        let info = make_info_full();
        let strategy = BaseOnlyStrategy;
        assert_eq!(strategy.pipeline_name(&info), "rust-ci");
    }
}
