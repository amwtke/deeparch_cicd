pub mod checkstyle;
pub mod pmd;
pub mod spotbugs;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::builder::{PipelineStrategy, StepDef};
use crate::ci::builder::base::BaseStrategy;

pub struct GradleStrategy;

impl GradleStrategy {
    fn parse_gradle_test(output: &str) -> Option<String> {
        let re = Regex::new(r"(\d+) tests completed, (\d+) failed").unwrap();
        let cap = re.captures(output)?;
        let total: u32 = cap[1].parse().unwrap_or(0);
        let failed: u32 = cap[2].parse().unwrap_or(0);
        let skipped_re = Regex::new(r"(\d+) skipped").unwrap();
        let skipped: u32 = skipped_re
            .captures(output)
            .and_then(|c| c[1].parse().ok())
            .unwrap_or(0);
        let passed = total.saturating_sub(failed + skipped);
        Some(format!("{} passed, {} failed, {} skipped", passed, failed, skipped))
    }
}

impl PipelineStrategy for GradleStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "gradle-java-ci".into()
    }

    fn output_report_str(&self, step_name: &str, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        match step_name {
            "test" => Self::parse_gradle_test(&output)
                .unwrap_or_else(|| BaseStrategy::default_report_str(step_name, success, stdout, stderr)),
            _ => BaseStrategy::default_report_str(step_name, success, stdout, stderr),
        }
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let cache_volumes = vec!["~/.gradle:/root/.gradle".to_string()];

        let mut steps = vec![BaseStrategy::build_step(info)];
        let mut quality_step_names: Vec<String> = vec![];
        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
            quality_step_names.push("checkstyle".into());
        }
        if info.quality_plugins.contains(&"spotbugs".to_string()) {
            steps.push(spotbugs::step(info));
            quality_step_names.push("spotbugs".into());
        }
        if info.quality_plugins.contains(&"pmd".to_string()) {
            steps.push(pmd::step(info));
            quality_step_names.push("pmd".into());
        }
        // Test depends on all quality checks
        let mut test_step = BaseStrategy::test_step(info);
        if !quality_step_names.is_empty() {
            test_step.depends_on = quality_step_names;
        }
        steps.push(test_step);

        for step in &mut steps {
            step.volumes = cache_volumes.clone();
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_gradle_info_with_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8.5-jdk17".into(),
            build_cmd: vec!["./gradlew build -x test".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: Some(vec!["./gradlew check -x test".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_gradle_info_without_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8.5-jdk17".into(),
            build_cmd: vec!["./gradlew build -x test".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_gradle_steps_with_checkstyle() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "checkstyle");
        assert_eq!(steps[2].name, "test");
    }

    #[test]
    fn test_gradle_steps_without_lint() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
    }

    #[test]
    fn test_gradle_pipeline_name() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        assert_eq!(strategy.pipeline_name(&info), "gradle-java-ci");
    }

    #[test]
    fn test_parse_test_output_basic() {
        let output = "10 tests completed, 0 failed";
        let strategy = GradleStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "10 passed, 0 failed, 0 skipped");
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "15 tests completed, 3 failed";
        let strategy = GradleStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "12 passed, 3 failed, 0 skipped");
    }

    #[test]
    fn test_parse_test_output_with_skipped() {
        let output = "20 tests completed, 1 failed, 2 skipped";
        let strategy = GradleStrategy;
        let report = strategy.output_report_str("test", false, output, "");
        assert_eq!(report, "17 passed, 1 failed, 2 skipped");
    }

    #[test]
    fn test_parse_test_output_no_match() {
        let output = "BUILD SUCCESSFUL";
        let strategy = GradleStrategy;
        let report = strategy.output_report_str("test", true, output, "");
        assert_eq!(report, "Tests passed");
    }
}
