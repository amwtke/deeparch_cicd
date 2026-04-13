pub mod build_step;
pub mod hygiene_step;
pub mod lint_step;
pub mod test_step;
pub mod typecheck_step;

use crate::ci::detector::ProjectInfo;
use crate::ci::parser::Pipeline;
use crate::ci::pipeline_builder::{test_parser, PipelineStrategy, StepDef};
use regex::Regex;
use std::collections::BTreeSet;

use build_step::VueBuildStep;
use lint_step::VueLintStep;
use test_step::VueTestStep;
use typecheck_step::VueTypecheckStep;

pub struct VueStrategy;

fn dedup_paths(paths: impl IntoIterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = paths.into_iter().collect();
    set.into_iter().collect()
}

fn vue_lint_context_paths(info: &ProjectInfo) -> Vec<String> {
    dedup_paths(
        info.source_paths
            .iter()
            .cloned()
            .chain(info.config_files.iter().cloned())
            .chain(
                [
                    "tests/",
                    "package.json",
                    "package-lock.json",
                    ".eslintrc.cjs",
                    ".eslintrc.js",
                    "vue.config.js",
                ]
                .iter()
                .map(|s| (*s).to_string()),
            ),
    )
}

fn vue_test_context_paths(info: &ProjectInfo) -> Vec<String> {
    dedup_paths(
        info.source_paths
            .iter()
            .cloned()
            .chain(info.config_files.iter().cloned())
            .chain(
                [
                    "tests/",
                    "package.json",
                    "package-lock.json",
                    "jest.config.js",
                    "vitest.config.js",
                    "vitest.config.ts",
                    "vue.config.js",
                ]
                .iter()
                .map(|s| (*s).to_string()),
            ),
    )
}

fn vue_typecheck_context_paths(info: &ProjectInfo) -> Vec<String> {
    dedup_paths(
        info.source_paths
            .iter()
            .cloned()
            .chain(info.config_files.iter().cloned())
            .chain(
                [
                    "tests/",
                    "package.json",
                    "package-lock.json",
                    "tsconfig.json",
                    "tsconfig.app.json",
                    "tsconfig.node.json",
                    "env.d.ts",
                    "src/env.d.ts",
                    "vue.config.js",
                    "vite.config.ts",
                    "vite.config.js",
                ]
                .iter()
                .map(|s| (*s).to_string()),
            ),
    )
}

fn vue_build_context_paths(info: &ProjectInfo) -> Vec<String> {
    dedup_paths(
        info.source_paths
            .iter()
            .cloned()
            .chain(info.config_files.iter().cloned())
            .chain(
                [
                    "public/",
                    "package.json",
                    "package-lock.json",
                    "vue.config.js",
                    "babel.config.js",
                    "vite.config.js",
                    "vite.config.ts",
                ]
                .iter()
                .map(|s| (*s).to_string()),
            ),
    )
}

fn parse_vue_test_summary(output: &str) -> Option<test_parser::TestSummary> {
    let jest_re = Regex::new(r"Tests:\s+(?:(\d+) failed,\s*)?(\d+) passed").unwrap();
    if let Some(cap) = jest_re.captures(output) {
        let failed: u32 = cap
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let passed: u32 = cap[2].parse().unwrap_or(0);
        return Some(test_parser::TestSummary {
            passed,
            failed,
            skipped: 0,
        });
    }
    let mocha_re = Regex::new(r"(\d+) passing").unwrap();
    if let Some(cap) = mocha_re.captures(output) {
        let passed: u32 = cap[1].parse().unwrap_or(0);
        return Some(test_parser::TestSummary {
            passed,
            failed: 0,
            skipped: 0,
        });
    }
    None
}

/// One-line summary for Jest / Mocha output (used by VueTestStep report).
fn parse_vue_test_line(output: &str) -> Option<String> {
    let s = parse_vue_test_summary(output)?;
    Some(format!("Tests: {} passed, {} failed", s.passed, s.failed))
}

impl PipelineStrategy for VueStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "vue-cli-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut out: Vec<Box<dyn StepDef>> = vec![];
        let mut prev_dep: Vec<String> = vec![];

        if let Some(tc) = VueTypecheckStep::new(info, prev_dep.clone()) {
            out.push(Box::new(tc));
            prev_dep = vec!["typecheck".into()];
        }

        if let Some(lint) = VueLintStep::new(info, prev_dep.clone()) {
            out.push(Box::new(lint));
            prev_dep = vec!["lint".into()];
        }

        out.push(Box::new(VueTestStep::new(info, prev_dep)));
        out.push(Box::new(VueBuildStep::new(info, vec!["test".into()])));

        out
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
            if let Some(line) = parse_vue_test_line(&output) {
                return line;
            }
        }
        if step_name == "typecheck" {
            let output = format!("{}{}", stdout, stderr);
            let errors = crate::ci::pipeline_builder::count_pattern(
                &output,
                &["error TS", "error:", "Error"],
            );
            if success {
                return "typecheck: passed".into();
            } else if errors > 0 {
                return format!("typecheck: {} errors", errors);
            }
            return "typecheck: failed".into();
        }
        crate::ci::pipeline_builder::base::BaseStrategy::default_report_str(
            step_name, success, stdout, stderr,
        )
    }

    fn parse_test_output(&self, output: &str) -> Option<test_parser::TestSummary> {
        parse_vue_test_summary(output)
    }

    fn steps_from_pipeline(
        &self,
        info: &ProjectInfo,
        pipeline: &crate::ci::parser::Pipeline,
    ) -> Vec<Box<dyn super::StepDef>> {
        step_defs_for_reconstructed_pipeline(info, pipeline)
    }
}

/// Step defs when rebuilding from YAML: typecheck commands come from the saved step, not detector inference.
pub(crate) fn step_defs_for_reconstructed_pipeline(
    info: &ProjectInfo,
    pipeline: &Pipeline,
) -> Vec<Box<dyn StepDef>> {
    let mut out: Vec<Box<dyn StepDef>> = vec![];
    let mut prev_dep: Vec<String> = vec![];

    if let Some(tc) = pipeline.get_step("typecheck") {
        out.push(Box::new(VueTypecheckStep::from_stored_commands(
            info,
            prev_dep.clone(),
            tc.commands.clone(),
        )));
        prev_dep = vec!["typecheck".into()];
    }

    if let Some(lint) = VueLintStep::new_from_stored_pipeline(info, prev_dep.clone()) {
        out.push(Box::new(lint));
        prev_dep = vec!["lint".into()];
    }

    out.push(Box::new(VueTestStep::new_from_stored_pipeline(
        info, prev_dep,
    )));
    out.push(Box::new(VueBuildStep::new(info, vec!["test".into()])));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn sample_vue_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Vue,
            language_version: Some("20".into()),
            framework: Some("vue".into()),
            image: "node:20-slim".into(),
            build_cmd: vec!["npm ci".into(), "npm run build".into()],
            test_cmd: vec![
                "npm ci".into(),
                "CI=true npm run test:unit -- --watchAll=false".into(),
            ],
            lint_cmd: Some(vec!["npm ci".into(), "npm run lint".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into(), "tests/".into()],
            config_files: vec![
                "package.json".into(),
                "package-lock.json".into(),
                "tsconfig.json".into(),
            ],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn vue_steps_with_hygiene_typecheck_lint_order() {
        let info = sample_vue_info();
        let steps = PipelineStrategy::steps(&VueStrategy, &info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["typecheck", "lint", "test", "build"]);
        assert!(steps[0].config().depends_on.is_empty());
        assert_eq!(steps[1].config().depends_on, vec!["typecheck"]);
        assert_eq!(steps[2].config().depends_on, vec!["lint"]);
        assert_eq!(steps[3].config().depends_on, vec!["test"]);
        let tc_cmds = &steps[0].config().commands;
        assert!(
            tc_cmds[0].contains("package-lock.json"),
            "first step should run lockfile/.nvmrc hygiene"
        );
    }

    #[test]
    fn vue_steps_without_lint_or_typecheck() {
        let mut info = sample_vue_info();
        info.lint_cmd = None;
        info.config_files.retain(|f| f != "tsconfig.json");
        let steps = PipelineStrategy::steps(&VueStrategy, &info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["test", "build"]);
        assert!(steps[0].config().depends_on.is_empty());
        assert!(
            steps[0].config().commands[0].contains("package-lock.json"),
            "test prepends hygiene when it is the first step"
        );
        assert_eq!(steps[1].config().depends_on, vec!["test"]);
    }

    #[test]
    fn parse_jest_summary() {
        let out = "Tests:  1 passed, 0 failed";
        let s = parse_vue_test_summary(out).unwrap();
        assert_eq!(s.passed, 1);
        assert_eq!(s.failed, 0);
    }
}
