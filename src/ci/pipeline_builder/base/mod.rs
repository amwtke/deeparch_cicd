pub mod build_step;
pub mod fmt_step;
pub mod git_diff_step;
pub mod git_pull_step;
pub mod jacoco_agent_decorator;
pub mod lint_step;
pub mod ping_pong_step;
pub mod test_step;

pub use build_step::BuildStep;
pub use fmt_step::FmtStep;
pub use git_diff_step::GitDiffStep;
pub use git_pull_step::GitPullStep;
pub use jacoco_agent_decorator::{JacocoAgentTestStep, JacocoMode, JACOCO_VERSION};
pub use lint_step::LintStep;
pub use ping_pong_step::PingPongStep;
pub use test_step::TestStep;

use super::count_pattern;

pub struct BaseStrategy;

impl BaseStrategy {
    /// Default report string for common steps. Language strategies can override
    /// specific steps and delegate the rest here.
    pub fn default_report_str(
        step_name: &str,
        success: bool,
        stdout: &str,
        stderr: &str,
    ) -> String {
        let output = format!("{}{}", stdout, stderr);
        match step_name {
            "ping-pong" => {
                if success {
                    "Ping-pong completed (10 rounds)".into()
                } else {
                    output
                        .lines()
                        .find(|l| l.contains("ping (round"))
                        .unwrap_or("ping")
                        .trim()
                        .into()
                }
            }
            "git-pull" => Self::report_git_pull(&output),
            "git-diff" => Self::report_git_diff(&output, success),
            "build" => Self::report_build(success, &output),
            "test" => Self::report_test_generic(success, &output),
            "lint" | "clippy" | "checkstyle" | "vet" => {
                Self::report_lint(success, step_name, &output)
            }
            "fmt-check" | "typecheck" | "mypy" => Self::report_check(success, step_name, &output),
            "spotbugs" | "spotbugs_full" => Self::report_spotbugs(step_name, success, &output),
            "pmd" | "pmd_full" => Self::report_pmd(step_name, success, &output),
            "package" => Self::report_package(success, &output),
            _ => {
                if success {
                    "OK".into()
                } else {
                    "Failed (exit non-zero)".into()
                }
            }
        }
    }

    fn report_git_diff(output: &str, success: bool) -> String {
        if output.contains("not a git repository") {
            return "git-diff: skipped (no git repo)".into();
        }
        if output.contains("working tree clean") {
            return "git-diff: skipped (tree clean)".into();
        }
        if let Some(line) = output
            .lines()
            .find(|l| l.contains("change record(s) on current branch"))
        {
            return line.trim().to_string();
        }
        if success {
            "git-diff: ok".into()
        } else {
            "git-diff: failed".into()
        }
    }

    fn report_git_pull(output: &str) -> String {
        if output.contains("Already up to date") || output.contains("Already up-to-date") {
            "Already up to date".into()
        } else if output.contains("skipping") || output.contains("Skipping") {
            let line = output
                .lines()
                .find(|l| l.contains("skipping"))
                .unwrap_or("Skipped");
            line.trim().into()
        } else if output.contains("files changed") || output.contains("file changed") {
            output
                .lines()
                .find(|l| l.contains("files changed") || l.contains("file changed"))
                .unwrap_or("Pulled latest changes")
                .trim()
                .into()
        } else if output.contains("Pulling") {
            "Pulled latest changes".into()
        } else {
            "OK".into()
        }
    }

    fn report_build(success: bool, output: &str) -> String {
        let warning_count = count_pattern(output, &["warning:", "WARNING", "[WARNING]"]);
        if success {
            if warning_count > 0 {
                format!("Build succeeded ({} warnings)", warning_count)
            } else {
                "Build succeeded".into()
            }
        } else {
            let error_count = count_pattern(output, &["error:", "ERROR", "[ERROR]"]);
            if error_count > 0 {
                format!("Build failed ({} errors)", error_count)
            } else {
                "Build failed".into()
            }
        }
    }

    fn report_test_generic(success: bool, _output: &str) -> String {
        if success {
            "Tests passed".into()
        } else {
            "Tests failed".into()
        }
    }

    fn report_lint(success: bool, name: &str, output: &str) -> String {
        let warning_count = count_pattern(output, &["warning:", "WARNING", "[WARN]"]);
        let violation_count = count_pattern(output, &["violation", "Violation"]);
        let issues = warning_count + violation_count;
        if success {
            if issues > 0 {
                format!("{}: passed ({} warnings)", name, issues)
            } else {
                format!("{}: no issues found", name)
            }
        } else if issues > 0 {
            format!("{}: {} issues found", name, issues)
        } else {
            format!("{}: failed", name)
        }
    }

    fn report_check(success: bool, name: &str, output: &str) -> String {
        if success {
            format!("{}: passed", name)
        } else {
            let error_count = count_pattern(output, &["error:", "Error"]);
            if error_count > 0 {
                format!("{}: {} errors", name, error_count)
            } else {
                format!("{}: failed", name)
            }
        }
    }

    fn report_spotbugs(step_name: &str, success: bool, output: &str) -> String {
        if output.contains("not a git repository") {
            return format!("{}: skipped (no git repo)", step_name);
        }
        if output.contains("no changed java files")
            || output.contains("changed java files have no matching compiled classes")
        {
            return format!("{}: skipped (no changed files)", step_name);
        }
        if let Some(line) = output.lines().find(|l| l.contains("SpotBugs Total:")) {
            return line.trim().to_string();
        }
        if !success {
            format!("{}: failed", step_name)
        } else {
            format!("{}: no bugs found", step_name)
        }
    }

    fn report_pmd(step_name: &str, success: bool, output: &str) -> String {
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            return format!("{}: ruleset not found (callback)", step_name);
        }
        if output.contains("not a git repository") {
            return format!("{}: skipped (no git repo)", step_name);
        }
        if output.contains("no changed source files") {
            return format!("{}: skipped (no changed files)", step_name);
        }
        if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
            return line.trim().to_string();
        }
        let violation_count = count_pattern(output, &["violation", "Violation"]);
        if !success && violation_count == 0 {
            format!("{}: failed", step_name)
        } else if violation_count > 0 {
            format!("{}: {} violations", step_name, violation_count)
        } else {
            format!("{}: no violations", step_name)
        }
    }

    fn report_package(success: bool, _output: &str) -> String {
        if success {
            "Package created".into()
        } else {
            "Package failed".into()
        }
    }
}
