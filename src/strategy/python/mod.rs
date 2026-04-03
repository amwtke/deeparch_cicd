pub mod mypy;

use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;

pub struct PythonStrategy;

impl PipelineStrategy for PythonStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "python-ci".into()
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
    use crate::detector::{ProjectInfo, ProjectType};

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
}
