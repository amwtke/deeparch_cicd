pub mod build_step;
pub mod fmt_step;
pub mod git_pull_step;
pub mod lint_step;
pub mod test_step;

pub use build_step::BuildStep;
pub use fmt_step::FmtStep;
pub use git_pull_step::GitPullStep;
pub use lint_step::LintStep;
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
            "git-pull" => Self::report_git_pull(&output),
            "build" => Self::report_build(success, &output),
            "test" => Self::report_test_generic(success, &output),
            "lint" | "clippy" | "checkstyle" | "vet" => {
                Self::report_lint(success, step_name, &output)
            }
            "fmt-check" | "typecheck" | "mypy" => Self::report_check(success, step_name, &output),
            "spotbugs" => Self::report_spotbugs(success, &output),
            "pmd" => Self::report_pmd(success, &output),
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

    fn report_spotbugs(success: bool, output: &str) -> String {
        let bug_count = count_pattern(output, &["Bug", "bug"]);
        if success {
            "spotbugs: no bugs found".into()
        } else if bug_count > 0 {
            format!("spotbugs: {} bugs found", bug_count)
        } else {
            "spotbugs: failed".into()
        }
    }

    fn report_pmd(success: bool, output: &str) -> String {
        // Extract "PMD Total: N violations" summary line if present
        if let Some(line) = output.lines().find(|l| l.contains("PMD Total:")) {
            return line.trim().to_string();
        }
        let violation_count = count_pattern(output, &["violation", "Violation"]);
        if !success && violation_count == 0 {
            "pmd: failed".into()
        } else if violation_count > 0 {
            format!("pmd: {} violations (report only)", violation_count)
        } else {
            "pmd: no violations".into()
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
