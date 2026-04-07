pub mod checkstyle;
pub mod package;
pub mod pmd;
pub mod spotbugs;

use regex::Regex;
use crate::detector::ProjectInfo;
use crate::pipeline_gen::{PipelineStrategy, StepDef};
use crate::pipeline_gen::base::BaseStrategy;
use crate::pipeline_gen::test_parser::TestSummary;

pub struct MavenStrategy;

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let re = Regex::new(r"Tests run: (\d+), Failures: (\d+), Errors: (\d+), Skipped: (\d+)")
            .unwrap();
        let mut total_run: u32 = 0;
        let mut total_failures: u32 = 0;
        let mut total_errors: u32 = 0;
        let mut total_skipped: u32 = 0;
        let mut found = false;
        for cap in re.captures_iter(output) {
            found = true;
            total_run += cap[1].parse::<u32>().unwrap_or(0);
            total_failures += cap[2].parse::<u32>().unwrap_or(0);
            total_errors += cap[3].parse::<u32>().unwrap_or(0);
            total_skipped += cap[4].parse::<u32>().unwrap_or(0);
        }
        if !found {
            return None;
        }
        let passed = total_run.saturating_sub(total_failures + total_errors + total_skipped);
        Some(TestSummary::new(passed, total_failures + total_errors, total_skipped))
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let cache_volumes = vec!["~/.m2:/root/.m2".to_string()];

        let mut steps = vec![BaseStrategy::build_step(info)];
        // Quality checks run after build, before test
        let mut quality_step_names: Vec<String> = vec![];
        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
            quality_step_names.push("checkstyle".into());
        }
        steps.push(spotbugs::step(info));
        quality_step_names.push("spotbugs".into());
        steps.push(pmd::step(info));
        quality_step_names.push("pmd".into());

        // Test depends on all quality checks (not just build)
        let mut test_step = BaseStrategy::test_step(info);
        test_step.depends_on = quality_step_names;
        steps.push(test_step);
        steps.push(package::step(info));

        // Mount Maven cache for all steps
        for step in &mut steps {
            step.volumes = cache_volumes.clone();
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::{ProjectInfo, ProjectType};

    fn make_maven_info_with_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: Some("spring-boot 3.2.0".into()),
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: Some(vec!["mvn checkstyle:check".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_maven_info_without_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_maven_steps_with_checkstyle() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        // build, checkstyle, spotbugs, pmd, test, package
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "checkstyle");
        assert_eq!(steps[2].name, "spotbugs");
        assert_eq!(steps[3].name, "pmd");
        assert_eq!(steps[4].name, "test");
        assert_eq!(steps[5].name, "package");
        // test depends on all quality steps
        assert_eq!(steps[4].depends_on, vec!["checkstyle", "spotbugs", "pmd"]);
    }

    #[test]
    fn test_maven_steps_without_checkstyle() {
        let info = make_maven_info_without_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        // build, spotbugs, pmd, test, package (no checkstyle)
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "spotbugs");
        assert_eq!(steps[2].name, "pmd");
        assert_eq!(steps[3].name, "test");
        // test depends on spotbugs and pmd (no checkstyle)
        assert_eq!(steps[3].depends_on, vec!["spotbugs", "pmd"]);
        assert_eq!(steps[4].name, "package");
    }

    #[test]
    fn test_maven_pipeline_name() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        assert_eq!(strategy.pipeline_name(&info), "maven-java-ci");
    }

    #[test]
    fn test_package_depends_on_test() {
        let info = make_maven_info_with_lint();
        let step = package::step(&info);
        assert_eq!(step.depends_on, vec!["test"]);
    }

    #[test]
    fn test_checkstyle_depends_on_build() {
        let info = make_maven_info_with_lint();
        let step = checkstyle::step(&info);
        assert_eq!(step.depends_on, vec!["build"]);
    }

    #[test]
    fn test_parse_test_output_single_module() {
        let output = "Tests run: 42, Failures: 0, Errors: 0, Skipped: 2";
        let strategy = MavenStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 40);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_parse_test_output_multi_module() {
        let output = "\
Tests run: 10, Failures: 1, Errors: 0, Skipped: 0
Tests run: 20, Failures: 0, Errors: 2, Skipped: 1
Tests run: 5, Failures: 0, Errors: 0, Skipped: 0";
        let strategy = MavenStrategy;
        let summary = strategy.parse_test_output(output).unwrap();
        // total_run=35, failures=1, errors=2, skipped=1 => passed=35-3-1=31, failed=3
        assert_eq!(summary.passed, 31);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_parse_test_output_no_tests() {
        let output = "BUILD SUCCESS";
        let strategy = MavenStrategy;
        assert!(strategy.parse_test_output(output).is_none());
    }
}
