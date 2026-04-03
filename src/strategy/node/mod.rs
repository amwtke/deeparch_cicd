pub mod typecheck;

use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;

pub struct NodeStrategy;

impl NodeStrategy {
    fn is_typescript(info: &ProjectInfo) -> bool {
        info.config_files.iter().any(|f| f.contains("tsconfig"))
            || info
                .framework
                .as_deref()
                .map(|f| f.contains("next") || f.contains("angular"))
                .unwrap_or(false)
    }
}

impl PipelineStrategy for NodeStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "node-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];
        if NodeStrategy::is_typescript(info) {
            steps.push(typecheck::step(info));
        }
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

    fn make_node_typescript_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some("20".into()),
            framework: Some("next 14".into()),
            image: "node:20-alpine".into(),
            build_cmd: vec!["npm run build".into()],
            test_cmd: vec!["npm test".into()],
            lint_cmd: Some(vec!["npm run lint".into()]),
            fmt_cmd: Some(vec!["npx prettier --check .".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["package.json".into(), "tsconfig.json".into()],
            warnings: vec![],
        }
    }

    fn make_node_no_typescript_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some("18".into()),
            framework: None,
            image: "node:18-alpine".into(),
            build_cmd: vec!["npm run build".into()],
            test_cmd: vec!["npm test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["package.json".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_node_typescript_steps() {
        let info = make_node_typescript_info();
        let strategy = NodeStrategy;
        let steps = strategy.steps(&info);
        // build, typecheck, lint, test, fmt-check
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "typecheck");
        assert_eq!(steps[2].name, "lint");
        assert_eq!(steps[3].name, "test");
        assert_eq!(steps[4].name, "fmt-check");
    }

    #[test]
    fn test_node_no_typescript() {
        let info = make_node_no_typescript_info();
        let strategy = NodeStrategy;
        let steps = strategy.steps(&info);
        // build, test only
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
    }

    #[test]
    fn test_node_pipeline_name() {
        let info = make_node_no_typescript_info();
        let strategy = NodeStrategy;
        assert_eq!(strategy.pipeline_name(&info), "node-ci");
    }
}
