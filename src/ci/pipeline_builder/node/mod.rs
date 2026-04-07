pub mod typecheck_step;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{PipelineStrategy, StepConfig, StepDef};
use crate::ci::pipeline_builder::base::{BuildStep, TestStep, LintStep, FmtStep};

pub struct NodeStrategy;

fn is_typescript(info: &ProjectInfo) -> bool {
    if info.config_files.iter().any(|f| f.contains("tsconfig")) {
        return true;
    }
    if let Some(ref framework) = info.framework {
        let fw = framework.to_lowercase();
        if fw.contains("next") || fw.contains("angular") {
            return true;
        }
    }
    false
}

fn parse_node_test(output: &str) -> Option<String> {
    // Jest format
    let jest_re = Regex::new(r"Tests:\s+(?:(\d+) failed,\s*)?(\d+) passed").unwrap();
    if let Some(cap) = jest_re.captures(output) {
        let failed: u32 = cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        let passed: u32 = cap[2].parse().unwrap_or(0);
        return Some(format!("{} passed, {} failed", passed, failed));
    }
    // Mocha format
    let mocha_re = Regex::new(r"(\d+) passing").unwrap();
    if let Some(cap) = mocha_re.captures(output) {
        let passed: u32 = cap[1].parse().unwrap_or(0);
        return Some(format!("{} passed", passed));
    }
    None
}

impl PipelineStrategy for NodeStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "node-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];

        // Build
        steps.push(Box::new(BuildStep::new(info)));

        // Typecheck (if TypeScript)
        if is_typescript(info) {
            steps.push(Box::new(typecheck_step::TypecheckStep::new(info)));
        }

        // Lint (optional)
        if let Some(lint_step) = LintStep::new(info) {
            steps.push(Box::new(lint_step));
        }

        // Test with node parser
        let test_step = TestStep::new(info).with_parser(parse_node_test);
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

    fn make_node_ts_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some("20".into()),
            framework: Some("next 14.0".into()),
            image: "node:20".into(),
            build_cmd: vec!["npm run build".into()],
            test_cmd: vec!["npm test".into()],
            lint_cmd: Some(vec!["npx eslint .".into()]),
            fmt_cmd: Some(vec!["npx prettier --check .".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["package.json".into(), "tsconfig.json".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_node_js_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some("20".into()),
            framework: None,
            image: "node:20".into(),
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
        let info = make_node_ts_info();
        let strategy = NodeStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "typecheck", "lint", "test", "fmt-check"]);
    }

    #[test]
    fn test_node_no_typescript_steps() {
        let info = make_node_js_info();
        let strategy = NodeStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "test"]);
    }

    #[test]
    fn test_node_pipeline_name() {
        let info = make_node_ts_info();
        assert_eq!(NodeStrategy.pipeline_name(&info), "node-ci");
    }

    #[test]
    fn test_parse_node_test_jest() {
        let output = "Tests:  2 failed, 10 passed";
        assert_eq!(parse_node_test(output).unwrap(), "10 passed, 2 failed");
    }

    #[test]
    fn test_parse_node_test_jest_all_pass() {
        let output = "Tests:  10 passed";
        assert_eq!(parse_node_test(output).unwrap(), "10 passed, 0 failed");
    }

    #[test]
    fn test_parse_node_test_mocha() {
        let output = "  5 passing (200ms)";
        assert_eq!(parse_node_test(output).unwrap(), "5 passed");
    }

    #[test]
    fn test_parse_node_test_no_match() {
        assert!(parse_node_test("npm run test exited").is_none());
    }
}
