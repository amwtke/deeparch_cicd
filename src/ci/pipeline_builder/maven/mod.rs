pub mod checkstyle_step;
pub mod package_step;
pub mod pmd_step;
pub mod spotbugs_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{self, BuildStep, TestStep};
use crate::ci::pipeline_builder::{test_parser, PipelineStrategy, StepConfig, StepDef};
use regex::Regex;

pub struct MavenStrategy;

fn parse_maven_test(output: &str) -> Option<String> {
    let re =
        Regex::new(r"Tests run: (\d+), Failures: (\d+), Errors: (\d+), Skipped: (\d+)").unwrap();
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
    let failed = total_failures + total_errors;
    Some(format!(
        "{} passed, {} failed, {} skipped",
        passed, failed, total_skipped
    ))
}

/// Wrapper that adds Maven cache volume to any step
struct MavenCachedStep {
    inner: Box<dyn StepDef>,
    depends_on_override: Option<Vec<String>>,
}

impl MavenCachedStep {
    fn wrap(inner: Box<dyn StepDef>) -> Self {
        Self {
            inner,
            depends_on_override: None,
        }
    }
    fn wrap_with_deps(inner: Box<dyn StepDef>, deps: Vec<String>) -> Self {
        Self {
            inner,
            depends_on_override: Some(deps),
        }
    }
}

impl StepDef for MavenCachedStep {
    fn config(&self) -> StepConfig {
        let mut cfg = self.inner.config();
        cfg.volumes = vec![
            "~/.m2:/workspace/.m2".to_string(),
            "~/.pipelight/cache:/workspace/.pipelight/cache".to_string(),
        ];
        if let Some(ref deps) = self.depends_on_override {
            cfg.depends_on = deps.clone();
        }
        cfg
    }
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        self.inner.output_report_str(success, stdout, stderr)
    }
}

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn output_report_str(
        &self,
        step_name: &str,
        success: bool,
        stdout: &str,
        stderr: &str,
    ) -> String {
        if step_name == "test" {
            let output = format!("{}{}", stdout, stderr);
            if let Some(summary) = parse_maven_test(&output) {
                return format!("Tests: {}", summary);
            }
        }
        base::BaseStrategy::default_report_str(step_name, success, stdout, stderr)
    }

    fn parse_test_output(&self, output: &str) -> Option<test_parser::TestSummary> {
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
        let failed = total_failures + total_errors;
        Some(test_parser::TestSummary {
            passed,
            failed,
            skipped: total_skipped,
        })
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];
        let mut quality_step_names: Vec<String> = vec![];

        // Build
        steps.push(Box::new(MavenCachedStep::wrap(Box::new(BuildStep::new(
            info,
        )))));

        // Quality checks
        if info.lint_cmd.is_some() {
            steps.push(Box::new(MavenCachedStep::wrap(Box::new(
                checkstyle_step::CheckstyleStep::new(info),
            ))));
            quality_step_names.push("checkstyle".into());
        }
        steps.push(Box::new(MavenCachedStep::wrap(Box::new(
            spotbugs_step::SpotbugsStep::new(info),
        ))));
        quality_step_names.push("spotbugs".into());
        steps.push(Box::new(MavenCachedStep::wrap(Box::new(
            pmd_step::PmdStep::new(info),
        ))));
        quality_step_names.push("pmd".into());

        // Test depends on quality steps
        let test_step = TestStep::new(info).with_parser(parse_maven_test);
        steps.push(Box::new(MavenCachedStep::wrap_with_deps(
            Box::new(test_step),
            quality_step_names,
        )));

        // Package
        steps.push(Box::new(MavenCachedStep::wrap(Box::new(
            package_step::PackageStep::new(info),
        ))));

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

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
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        // build, checkstyle, spotbugs, pmd, test, package
        assert_eq!(
            names,
            vec!["build", "checkstyle", "spotbugs", "pmd", "test", "package"]
        );
        // test depends on quality steps
        let test_cfg = steps[4].config();
        assert_eq!(test_cfg.depends_on, vec!["checkstyle", "spotbugs", "pmd"]);
    }

    #[test]
    fn test_maven_steps_without_checkstyle() {
        let info = make_maven_info_without_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "spotbugs", "pmd", "test", "package"]);
        let test_cfg = steps[3].config();
        assert_eq!(test_cfg.depends_on, vec!["spotbugs", "pmd"]);
    }

    #[test]
    fn test_maven_pipeline_name() {
        let info = make_maven_info_with_lint();
        assert_eq!(MavenStrategy.pipeline_name(&info), "maven-java-ci");
    }

    #[test]
    fn test_checkstyle_depends_on_build() {
        let info = make_maven_info_with_lint();
        let step = checkstyle_step::CheckstyleStep::new(&info);
        assert_eq!(step.config().depends_on, vec!["build"]);
    }

    #[test]
    fn test_package_depends_on_test() {
        let info = make_maven_info_with_lint();
        let step = package_step::PackageStep::new(&info);
        assert_eq!(step.config().depends_on, vec!["test"]);
    }

    #[test]
    fn test_maven_cache_volumes() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        for step in &steps {
            let cfg = step.config();
            assert!(
                cfg.volumes.contains(&"~/.m2:/workspace/.m2".to_string()),
                "step '{}' should have Maven cache volume",
                cfg.name
            );
        }
    }

    #[test]
    fn test_pmd_step_uses_auto_gen_strategy() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_maven_info_with_lint();
        let step = pmd_step::PmdStep::new(&info);
        let mapping = step.exception_mapping();
        let resolved = mapping.resolve(1, "", "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset", Some(&|ec, out, err| step.match_exception(ec, out, err)));
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(resolved.max_retries, 2);
    }

    #[test]
    fn test_pmd_step_command_has_callback() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(cmd.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset"));
        assert!(cmd.contains("pipelight-misc/pmd-ruleset.xml"));
    }

    #[test]
    fn test_pmd_step_detects_invalid_ruleset() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(
            cmd.contains("Cannot load ruleset"),
            "should detect ruleset loading errors"
        );
        assert!(
            cmd.contains("Unable to find referenced rule"),
            "should detect invalid rule names"
        );
        assert!(cmd.contains("exit 1"), "should exit 1 on invalid ruleset");
    }

    #[test]
    fn test_pmd_callback_includes_pmd_version() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(
            cmd.contains("PMD 7"),
            "callback message should mention PMD 7.x"
        );
        assert!(
            cmd.contains("not PMD 6"),
            "callback should warn against PMD 6.x rule names"
        );
    }

    #[test]
    fn test_spotbugs_step_uses_autofix() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_maven_info_with_lint();
        let step = spotbugs_step::SpotbugsStep::new(&info);
        let resolved = step.exception_mapping().resolve(1, "", "some spotbugs error", Some(&|ec, out, err| step.match_exception(ec, out, err)));
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
    }

    #[test]
    fn test_checkstyle_step_uses_autofix() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_maven_info_with_lint();
        let step = checkstyle_step::CheckstyleStep::new(&info);
        let resolved = step.exception_mapping().resolve(1, "", "some checkstyle error", Some(&|ec, out, err| step.match_exception(ec, out, err)));
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
    }

    #[test]
    fn test_package_step_uses_abort() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_maven_info_with_lint();
        let step = package_step::PackageStep::new(&info);
        let resolved = step.exception_mapping().resolve(1, "", "some package error", None);
        assert_eq!(resolved.command, CallbackCommand::RuntimeError);
    }

    #[test]
    fn test_parse_maven_test_single_module() {
        let output = "Tests run: 42, Failures: 0, Errors: 0, Skipped: 2";
        assert_eq!(
            parse_maven_test(output).unwrap(),
            "40 passed, 0 failed, 2 skipped"
        );
    }

    #[test]
    fn test_parse_maven_test_multi_module() {
        let output = "\
Tests run: 10, Failures: 1, Errors: 0, Skipped: 0
Tests run: 20, Failures: 0, Errors: 2, Skipped: 1
Tests run: 5, Failures: 0, Errors: 0, Skipped: 0";
        assert_eq!(
            parse_maven_test(output).unwrap(),
            "31 passed, 3 failed, 1 skipped"
        );
    }

    #[test]
    fn test_parse_maven_test_no_match() {
        assert!(parse_maven_test("BUILD SUCCESS").is_none());
    }
}
