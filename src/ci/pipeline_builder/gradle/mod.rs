pub mod checkstyle_step;
pub mod pmd_step;
pub mod spotbugs_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base;
use crate::ci::pipeline_builder::base::{BuildStep, TestStep};
use crate::ci::pipeline_builder::{test_parser, PipelineStrategy, StepConfig, StepDef};
use regex::Regex;

pub struct GradleStrategy;

fn parse_gradle_test(output: &str) -> Option<String> {
    // Gradle prints `N tests completed, M failed[, K skipped]` once per module
    // whenever that module has failing tests. With `--continue` every failing
    // module emits its own line, so we aggregate across all matches instead of
    // reading only the first.
    let re = Regex::new(r"(\d+) tests? completed(?:, (\d+) failed)?(?:, (\d+) skipped)?").unwrap();
    let (mut total, mut failed, mut skipped) = (0u32, 0u32, 0u32);
    let mut found = false;
    for cap in re.captures_iter(output) {
        found = true;
        total += cap[1].parse::<u32>().unwrap_or(0);
        failed += cap
            .get(2)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        skipped += cap
            .get(3)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
    }
    if !found {
        return None;
    }
    let passed = total.saturating_sub(failed + skipped);
    Some(format!(
        "{} passed, {} failed, {} skipped",
        passed, failed, skipped
    ))
}

/// Wrapper that adds Gradle cache volume to any step and optionally overrides
/// the step's `depends_on` so strategies can compose steps into a serial chain.
struct GradleCachedStep {
    inner: Box<dyn StepDef>,
    depends_on_override: Option<Vec<String>>,
}

impl GradleCachedStep {
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

impl StepDef for GradleCachedStep {
    fn config(&self) -> StepConfig {
        let mut cfg = self.inner.config();
        cfg.volumes = vec![
            "~/.gradle:/workspace/.gradle".to_string(),
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

    fn output_report_str(
        &self,
        step_name: &str,
        success: bool,
        stdout: &str,
        stderr: &str,
    ) -> String {
        if step_name == "test" {
            let output = format!("{}{}", stdout, stderr);
            if let Some(summary) = parse_gradle_test(&output) {
                return format!("Tests: {}", summary);
            }
            // Parser saw no per-module counts (e.g. all tests were cached or
            // nothing ran). When `--continue` aggregates failures, Gradle still
            // emits BUILD FAILED / FAILURE markers — surface that instead of
            // claiming "Tests passed".
            let looks_failed = output.contains("BUILD FAILED")
                || output.contains("FAILURE:")
                || output.contains("There were failing tests");
            if looks_failed {
                return "Tests had failures (report-only)".into();
            }
        }
        base::BaseStrategy::default_report_str(step_name, success, stdout, stderr)
    }

    fn parse_test_output(&self, output: &str) -> Option<test_parser::TestSummary> {
        let re =
            Regex::new(r"(\d+) tests? completed(?:, (\d+) failed)?(?:, (\d+) skipped)?").unwrap();
        let (mut total, mut failed, mut skipped) = (0u32, 0u32, 0u32);
        let mut found = false;
        for cap in re.captures_iter(output) {
            found = true;
            total += cap[1].parse::<u32>().unwrap_or(0);
            failed += cap
                .get(2)
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(0);
            skipped += cap
                .get(3)
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(0);
        }
        if !found {
            return None;
        }
        let passed = total.saturating_sub(failed + skipped);
        Some(test_parser::TestSummary {
            passed,
            failed,
            skipped,
        })
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        // Serial chain: build → (checkstyle →)? (spotbugs →)? pmd → test
        // Each step depends on the immediately preceding step, giving a simple,
        // predictable linear execution order. Failures in any step short-circuit
        // the pipeline (with per-step callback dispatch to the LLM).
        let mut steps: Vec<Box<dyn StepDef>> = vec![];
        let mut prev: String = "build".into();

        steps.push(Box::new(GradleCachedStep::wrap(Box::new(BuildStep::new(
            info,
        )))));

        if info.lint_cmd.is_some() {
            steps.push(Box::new(GradleCachedStep::wrap_with_deps(
                Box::new(checkstyle_step::CheckstyleStep::new(info)),
                vec![prev.clone()],
            )));
            prev = "checkstyle".into();
        }

        if info.quality_plugins.contains(&"spotbugs".to_string()) {
            steps.push(Box::new(GradleCachedStep::wrap_with_deps(
                Box::new(spotbugs_step::SpotbugsStep::new(info)),
                vec![prev.clone()],
            )));
            prev = "spotbugs".into();
        }

        // PMD always runs — uses init script to inject plugin if not configured.
        steps.push(Box::new(GradleCachedStep::wrap_with_deps(
            Box::new(pmd_step::PmdStep::new(info)),
            vec![prev.clone()],
        )));
        prev = "pmd".into();

        // Test step: run all tests (--continue), report-only (allow_failure),
        // no auto_fix — just produce the test report.
        let mut test_info = info.clone();
        test_info.test_cmd = info
            .test_cmd
            .iter()
            .map(|cmd| {
                if !cmd.contains("--continue") {
                    format!("{} --continue", cmd)
                } else {
                    cmd.clone()
                }
            })
            .collect();
        let test_step = TestStep::new(&test_info)
            .with_parser(parse_gradle_test)
            .with_allow_failure(true)
            .with_test_report_globs(vec![
                "**/build/test-results/test/*.xml".into(),
                "**/build/reports/tests/test/index.html".into(),
            ]);
        steps.push(Box::new(GradleCachedStep::wrap_with_deps(
            Box::new(test_step),
            vec![prev],
        )));

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
        // Serial chain: each step depends on the immediately preceding one.
        let by_name: std::collections::HashMap<String, Vec<String>> = steps
            .iter()
            .map(|s| {
                let c = s.config();
                (c.name, c.depends_on)
            })
            .collect();
        assert_eq!(by_name["checkstyle"], vec!["build".to_string()]);
        assert_eq!(by_name["spotbugs"], vec!["checkstyle".to_string()]);
        assert_eq!(by_name["pmd"], vec!["spotbugs".to_string()]);
        assert_eq!(by_name["test"], vec!["pmd".to_string()]);
    }

    #[test]
    fn test_gradle_steps_without_lint() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        // PMD is always present even without lint plugins
        assert_eq!(names, vec!["build", "pmd", "test"]);
        // Serial chain collapses cleanly when quality plugins are absent.
        let pmd_cfg = steps[1].config();
        let test_cfg = steps[2].config();
        assert_eq!(pmd_cfg.depends_on, vec!["build".to_string()]);
        assert_eq!(test_cfg.depends_on, vec!["pmd".to_string()]);
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
        assert!(of.exceptions.contains_key("pmd_violations"));
        assert!(of.exceptions.contains_key("ruleset_not_found"));
        assert!(of.exceptions.contains_key("ruleset_invalid"));
        let pv = &of.exceptions["pmd_violations"];
        assert_eq!(pv.command, CallbackCommand::PmdPrintCommand);
        assert_eq!(pv.max_retries, 0);
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
    fn test_pmd_step_uses_standalone_cli() {
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
            cmd.contains("pmd check"),
            "should use standalone pmd check command"
        );
        assert!(
            cmd.contains("pmd-bin-"),
            "should download standalone PMD CLI"
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
    }

    #[test]
    fn test_spotbugs_step_uses_spotbugs_print() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = spotbugs_step::SpotbugsStep::new(&info);
        let resolved = step.exception_mapping().resolve(
            1,
            "SpotBugs Total: 3 bugs found\n",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::SpotbugsPrintCommand);
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
    fn test_pmd_step_reports_to_pipelight_misc() {
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
            cmd.contains("pipelight-misc/pmd-report"),
            "should write reports to pipelight-misc"
        );
        assert!(
            cmd.contains("pmd-summary.txt"),
            "should generate summary text file"
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
    fn test_pmd_incremental_scan_uses_git_diff() {
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
            cmd.contains("git diff --relative --name-only"),
            "should collect unstaged working tree changes"
        );
        assert!(
            cmd.contains("git diff --cached --relative --name-only"),
            "should collect staged source files"
        );
        assert!(
            cmd.contains("\"$UPSTREAM\"..HEAD"),
            "should collect unpushed commits via upstream diff"
        );
        assert!(
            cmd.contains("'*.java' '*.kt'"),
            "should scan both .java and .kt files"
        );
        assert!(
            cmd.contains("@{upstream}"),
            "should detect upstream of current branch"
        );
        assert!(
            cmd.contains("no changed source files"),
            "should skip when no changed files"
        );
        assert!(
            cmd.contains("FULL_SCAN=1") && cmd.contains("src/main/kotlin"),
            "should fall back to full scan when not a git repo"
        );
        assert!(
            cmd.contains("report-only"),
            "full-scan mode should be report-only"
        );
    }

    #[test]
    fn test_pmd_violation_triggers_pmd_print() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let step = pmd_step::PmdStep::new(&info);
        let mapping = step.exception_mapping();
        // PMD violations → pmd_print_command (report-only, no retry)
        let resolved = mapping.resolve(
            1,
            "PMD Total: 5 violations",
            "some pmd output",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::PmdPrintCommand);
        assert_eq!(resolved.max_retries, 0);
        assert_eq!(resolved.exception_key, "pmd_violations");
    }

    #[test]
    fn test_pmd_report_no_changed_files() {
        let pmd = pmd_step::PmdStep::new(&make_gradle_info_with_lint());
        let report = pmd.output_report_str(
            true,
            "PMD: no changed source files on current branch — skipping",
            "",
        );
        assert_eq!(report, "pmd: skipped (no changed files)");
    }

    #[test]
    fn test_test_step_allow_failure() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let test_cfg = steps
            .iter()
            .find(|s| s.config().name == "test")
            .unwrap()
            .config();
        assert!(
            test_cfg.allow_failure,
            "test step should have allow_failure=true"
        );
    }

    #[test]
    fn test_test_step_uses_continue_flag() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let test_cfg = steps
            .iter()
            .find(|s| s.config().name == "test")
            .unwrap()
            .config();
        assert!(
            test_cfg.commands.iter().any(|c| c.contains("--continue")),
            "test command should include --continue flag"
        );
    }

    #[test]
    fn test_test_step_no_autofix() {
        use crate::ci::callback::command::CallbackCommand;
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let step_defs = strategy.steps(&info);
        let test_step = step_defs
            .iter()
            .find(|s| s.config().name == "test")
            .unwrap();
        let resolved = test_step
            .exception_mapping()
            .resolve(1, "", "test failure", None);
        assert_eq!(
            resolved.command,
            CallbackCommand::Abort,
            "test failures should abort, not auto_fix"
        );
        assert_eq!(resolved.max_retries, 0, "test should have 0 retries");
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
