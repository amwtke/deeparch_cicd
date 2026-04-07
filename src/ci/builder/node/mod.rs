pub mod typecheck;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::builder::{PipelineStrategy, StepDef};
use crate::ci::builder::base::BaseStrategy;

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

    fn parse_node_test(output: &str) -> Option<String> {
        // Try Jest format first
        let jest_re = Regex::new(r"Tests:\s+(?:(\d+) failed,\s*)?(\d+) passed").unwrap();
        if let Some(cap) = jest_re.captures(output) {
            let failed: u32 = cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let passed: u32 = cap[2].parse().unwrap_or(0);
            return Some(format!("{} passed, {} failed", passed, failed));
        }
        // Fallback: Mocha format
        let mocha_re = Regex::new(r"(\d+) passing").unwrap();
        if let Some(cap) = mocha_re.captures(output) {
            let passed: u32 = cap[1].parse().unwrap_or(0);
            return Some(format!("{} passed", passed));
        }
        None
    }
}

impl PipelineStrategy for NodeStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "node-ci".into()
    }

    fn output_report_str(&self, step_name: &str, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        match step_name {
            "test" => Self::parse_node_test(&output)
                .unwrap_or_else(|| BaseStrategy::default_report_str(step_name, success, stdout, stderr)),
            _ => BaseStrategy::default_report_str(step_name, success, stdout, stderr),
        }
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
    use crate::ci::detector::{ProjectInfo, ProjectType};

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
            quality_plugins: vec![],
            subdir: None,
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
            quality_plugins: vec![],
            subdir: None,
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

    #[test]
    fn test_parse_test_output_jest_with_failures() {
        let output = "Tests: 3 failed, 10 passed, 13 total";
        let strategy = NodeStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "10 passed, 3 failed");
    }

    #[test]
    fn test_parse_test_output_jest_pass_only() {
        let output = "Tests: 20 passed, 20 total";
        let strategy = NodeStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "20 passed, 0 failed");
    }

    #[test]
    fn test_parse_test_output_mocha_format() {
        let output = "  15 passing (2s)\n  1 failing";
        let strategy = NodeStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "15 passed");
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "npm run test\n> jest";
        let strategy = NodeStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "Tests passed");
    }
}
