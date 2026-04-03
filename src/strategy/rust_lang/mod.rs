pub mod clippy;

use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;

pub struct RustStrategy;

impl PipelineStrategy for RustStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "rust-ci".into()
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
}
