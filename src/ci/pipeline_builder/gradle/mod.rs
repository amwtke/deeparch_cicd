pub mod checkstyle_step;
pub mod pmd_step;
pub mod spotbugs_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{BuildStep, TestStep};
use crate::ci::pipeline_builder::{PipelineStrategy, StepConfig, StepDef};
use regex::Regex;

pub struct GradleStrategy;

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
    Some(format!(
        "{} passed, {} failed, {} skipped",
        passed, failed, skipped
    ))
}

/// Wrapper that adds Gradle cache volume to any step
struct GradleCachedStep {
    inner: Box<dyn StepDef>,
}

impl GradleCachedStep {
    fn wrap(inner: Box<dyn StepDef>) -> Self {
        Self { inner }
    }
}

impl StepDef for GradleCachedStep {
    fn config(&self) -> StepConfig {
        let mut cfg = self.inner.config();
        cfg.volumes = vec![
            "~/.gradle:/workspace/.gradle".to_string(),
            "~/.pipelight/cache:/workspace/.pipelight/cache".to_string(),
        ];
        cfg
    }
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        self.inner.output_report_str(success, stdout, stderr)
    }

    fn exception_mapping(&self) -> crate::ci::callback::exception::ExceptionMapping {
        self.inner.exception_mapping()
    }

    fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        self.inner.match_exception(exit_code, stdout, stderr)
    }
}

impl PipelineStrategy for GradleStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "gradle-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![];

        // Build
        steps.push(Box::new(GradleCachedStep::wrap(Box::new(BuildStep::new(
            info,
        )))));

        // Checkstyle (if lint_cmd present)
        if info.lint_cmd.is_some() {
            steps.push(Box::new(GradleCachedStep::wrap(Box::new(
                checkstyle_step::CheckstyleStep::new(info),
            ))));
        }

        // Spotbugs (if quality_plugins contains "spotbugs")
        if info.quality_plugins.contains(&"spotbugs".to_string()) {
            steps.push(Box::new(GradleCachedStep::wrap(Box::new(
                spotbugs_step::SpotbugsStep::new(info),
            ))));
        }

        // PMD (always — uses init script to inject plugin if not configured in build.gradle)
        steps.push(Box::new(GradleCachedStep::wrap(Box::new(
            pmd_step::PmdStep::new(info),
        ))));

        // Test depends only on build — quality steps (checkstyle/spotbugs/pmd) run in parallel
        // with test and must not block it, since code-quality failures are independent of
        // test verification.
        let test_step = TestStep::new(info).with_parser(parse_gradle_test);
        steps.push(Box::new(GradleCachedStep::wrap(Box::new(test_step))));

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
            framework: Some("spring-boot 3.2.0".into()),
            image: "gradle:8-jdk17".into(),
            build_cmd: vec![
                "./gradlew assemble --max-workers=2 --build-cache --configuration-cache".into(),
            ],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: Some(vec!["./gradlew check -x test".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec!["spotbugs".into(), "pmd".into()],
            subdir: None,
        }
    }

    fn make_gradle_info_without_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8-jdk17".into(),
            build_cmd: vec![
                "./gradlew assemble --max-workers=2 --build-cache --configuration-cache".into(),
            ],
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
    fn test_gradle_steps_with_lint() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(
            names,
            vec!["build", "checkstyle", "spotbugs", "pmd", "test"]
        );
        // test only depends on build — quality steps don't block test
        let test_cfg = steps[4].config();
        assert_eq!(test_cfg.depends_on, vec!["build"]);
    }

    #[test]
    fn test_gradle_steps_without_lint() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        // PMD is always present even without lint plugins
        assert_eq!(names, vec!["build", "pmd", "test"]);
        let test_cfg = steps[2].config();
        assert_eq!(test_cfg.depends_on, vec!["build"]);
    }

    #[test]
    fn test_gradle_quality_steps_parallel_with_test() {
        // Regression: pmd/spotbugs/checkstyle must not block test.
        // All quality steps and test should be siblings depending only on "build",
        // so a pmd failure cannot cause test to be skipped.
        let info = make_gradle_info_with_lint();
        let steps = GradleStrategy.steps(&info);
        for name in ["checkstyle", "spotbugs", "pmd", "test"] {
            let cfg = steps
                .iter()
                .find(|s| s.config().name == name)
                .unwrap_or_else(|| panic!("step {} missing", name))
                .config();
            assert_eq!(
                cfg.depends_on,
                vec!["build".to_string()],
                "step '{}' must depend only on build so pmd failure doesn't block test",
                name
            );
        }
    }

    #[test]
    fn test_gradle_pmd_and_test_are_siblings_without_lint() {
        // Without checkstyle/spotbugs, pmd and test still must run in parallel.
        let info = make_gradle_info_without_lint();
        let steps = GradleStrategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let test_cfg = steps
            .iter()
            .find(|s| s.config().name == "test")
            .unwrap()
            .config();
        assert_eq!(pmd_cfg.depends_on, vec!["build".to_string()]);
        assert_eq!(test_cfg.depends_on, vec!["build".to_string()]);
    }

    #[test]
    fn test_gradle_pipeline_name() {
        let info = make_gradle_info_with_lint();
        assert_eq!(GradleStrategy.pipeline_name(&info), "gradle-java-ci");
    }

    #[test]
    fn test_gradle_cache_volumes() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        for step in &steps {
            let cfg = step.config();
            assert!(
                cfg.volumes
                    .contains(&"~/.gradle:/workspace/.gradle".to_string()),
                "step '{}' should have Gradle cache volume",
                cfg.name
            );
            assert!(
                cfg.volumes
                    .contains(&"~/.pipelight/cache:/workspace/.pipelight/cache".to_string()),
                "step '{}' should have pipelight cache volume",
                cfg.name
            );
        }
    }

    #[test]
    fn test_pmd_step_ruleset_not_found_triggers_auto_gen() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = pmd_step::PmdStep::new(&info);
        let mapping = step.exception_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(resolved.max_retries, 2);
        assert!(!resolved.context_paths.is_empty());
    }

    #[test]
    fn test_pmd_step_ruleset_not_found_and_invalid_both_auto_gen() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = pmd_step::PmdStep::new(&info);
        let mapping = step.exception_mapping();

        // ruleset_not_found → AutoGenPmdRuleset (LLM searches for guidelines)
        let not_found = mapping.resolve(
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(not_found.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(not_found.max_retries, 2);

        // ruleset_invalid → AutoGenPmdRuleset (LLM regenerates with correct rule names)
        let invalid = mapping.resolve(
            1,
            "",
            "Unable to find referenced rule SomeRule",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(invalid.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(invalid.max_retries, 2);
    }

    #[test]
    fn test_pmd_step_to_on_failure_has_exceptions() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = pmd_step::PmdStep::new(&info);
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(of.callback_command, CallbackCommand::RuntimeError);
        assert!(of.exceptions.contains_key("ruleset_not_found"));
        assert!(of.exceptions.contains_key("ruleset_invalid"));
        let rnf = &of.exceptions["ruleset_not_found"];
        assert_eq!(rnf.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(rnf.max_retries, 2);
    }

    #[test]
    fn test_pmd_step_always_present_without_plugin() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        assert!(
            steps.iter().any(|s| s.config().name == "pmd"),
            "PMD step should always be present even without PMD plugin in build.gradle"
        );
    }

    #[test]
    fn test_pmd_step_has_standalone_fallback() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(
            cmd.contains("pmdMain --dry-run"),
            "should check if Gradle PMD plugin exists"
        );
        assert!(
            cmd.contains("pmd-init.gradle"),
            "should use init script when plugin exists"
        );
        assert!(
            cmd.contains("standalone PMD CLI"),
            "should fall back to standalone PMD"
        );
        assert!(
            cmd.contains("pmd check"),
            "should have standalone pmd check command"
        );
    }

    #[test]
    fn test_pmd_step_command_checks_ruleset() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(
            cmd.contains("pipelight-misc/pmd-ruleset.xml"),
            "should check for ruleset"
        );
        assert!(
            cmd.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset"),
            "should emit callback when no ruleset"
        );
        assert!(
            cmd.contains("pmd-init.gradle"),
            "should use init script for custom ruleset"
        );
    }

    #[test]
    fn test_spotbugs_step_uses_autofix() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = spotbugs_step::SpotbugsStep::new(&info);
        let resolved = step.exception_mapping().resolve(
            1,
            "",
            "some spotbugs error",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
    }

    #[test]
    fn test_checkstyle_step_uses_autofix() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = checkstyle_step::CheckstyleStep::new(&info);
        let resolved = step.exception_mapping().resolve(
            1,
            "",
            "some checkstyle error",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
    }

    #[test]
    fn test_pmd_step_collects_multimodule_reports() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let pmd_cfg = steps
            .iter()
            .find(|s| s.config().name == "pmd")
            .unwrap()
            .config();
        let cmd = &pmd_cfg.commands[0];
        assert!(
            cmd.contains("find . -path"),
            "should collect multi-module reports via find"
        );
        assert!(
            cmd.contains("pipelight-misc/pmd-report"),
            "should copy reports to pipelight-misc"
        );
    }

    #[test]
    fn test_pmd_step_detects_invalid_ruleset() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
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
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
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
    fn test_pmd_report_callback_message() {
        let pmd = pmd_step::PmdStep::new(&make_gradle_info_with_lint());
        let report = pmd.output_report_str(false, "", "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset");
        assert_eq!(report, "pmd: ruleset not found (callback)");
    }

    #[test]
    fn test_parse_gradle_test_basic() {
        let output = "10 tests completed, 0 failed";
        assert_eq!(
            parse_gradle_test(output).unwrap(),
            "10 passed, 0 failed, 0 skipped"
        );
    }

    #[test]
    fn test_parse_gradle_test_with_failures() {
        let output = "20 tests completed, 3 failed";
        assert_eq!(
            parse_gradle_test(output).unwrap(),
            "17 passed, 3 failed, 0 skipped"
        );
    }

    #[test]
    fn test_parse_gradle_test_with_skipped() {
        let output = "15 tests completed, 2 failed, 3 skipped";
        assert_eq!(
            parse_gradle_test(output).unwrap(),
            "10 passed, 2 failed, 3 skipped"
        );
    }

    #[test]
    fn test_parse_gradle_test_no_match() {
        assert!(parse_gradle_test("BUILD SUCCESSFUL").is_none());
    }
}
