use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::executor::{LogLine, LogStream, StepResult};
use crate::pipeline::Pipeline;
use crate::scheduler::Scheduler;
use crate::strategy::test_parser::TestSummary;

static ROCKET: Emoji<'_, '_> = Emoji("🚀 ", ">> ");
static CHECK: Emoji<'_, '_> = Emoji("✅ ", "[OK] ");
static CROSS: Emoji<'_, '_> = Emoji("❌ ", "[FAIL] ");
static ARROW: Emoji<'_, '_> = Emoji("▸ ", "> ");
static CLOCK: Emoji<'_, '_> = Emoji("⏱  ", "");
static PENDING: Emoji<'_, '_> = Emoji("⬚  ", "[ ] ");
static CHART: Emoji<'_, '_> = Emoji("📊 ", "");

/// Multi-step progress UI for TTY mode.
/// Uses a single spinner for the active step, prints completed steps as static lines.
pub struct PipelineProgressUI {
    pending_steps: Vec<String>,
    current_spinner: Option<ProgressBar>,
    verbose: bool,
}

impl PipelineProgressUI {
    pub fn new(step_names: &[String], verbose: bool) -> Self {
        Self {
            pending_steps: step_names.to_vec(),
            current_spinner: None,
            verbose,
        }
    }

    /// Print header and pending step list.
    pub fn print_header(&self, pipeline_name: &str, step_count: usize) {
        println!();
        println!(
            "{} {} {} ({} steps)",
            ROCKET,
            style("Pipeline:").bold(),
            style(pipeline_name).cyan().bold(),
            step_count
        );
        println!("{}", style("─".repeat(56)).dim());
    }

    /// Mark a step as started — show a spinner on that line.
    pub fn start_step(&mut self, name: &str) {
        // Stop any previous spinner cleanly
        if let Some(pb) = self.current_spinner.take() {
            pb.finish_and_clear();
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner} {msg}")
                .unwrap(),
        );
        pb.set_message(format!(
            "{} {}  Running...",
            style(name).bold(),
            style("0.0s").dim()
        ));
        pb.enable_steady_tick(Duration::from_millis(100));
        self.current_spinner = Some(pb);
    }

    /// Print a log line below the spinner (only stderr in non-verbose, all in verbose).
    pub fn print_log(&self, line: &LogLine) {
        if self.verbose || line.stream == LogStream::Stderr {
            let prefix = match line.stream {
                LogStream::Stdout => style("   │ ").dim(),
                LogStream::Stderr => style("   │ ").red().dim(),
            };
            if let Some(ref pb) = self.current_spinner {
                pb.suspend(|| {
                    print!("{}{}", prefix, line.message);
                });
            }
        }
    }

    /// Mark step as finished — clear spinner, print a static result line.
    pub fn finish_step(&mut self, name: &str, success: bool, duration: Duration) {
        // Clear the spinner
        if let Some(pb) = self.current_spinner.take() {
            pb.finish_and_clear();
        }

        let icon = if success {
            CHECK.to_string()
        } else {
            CROSS.to_string()
        };
        let name_styled = if success {
            style(name).green().bold().to_string()
        } else {
            style(name).red().bold().to_string()
        };
        println!(
            "{}{:<16} {}",
            icon,
            name_styled,
            style(format_duration(duration)).dim()
        );
    }

    /// Print test summary.
    pub fn print_test_summary(&self, summary: &TestSummary) {
        println!();
        let passed = style(format!("{} passed", summary.passed)).green();
        let failed = if summary.failed > 0 {
            style(format!("{} failed", summary.failed))
                .red()
                .to_string()
        } else {
            format!("{} failed", summary.failed)
        };
        let skipped = if summary.skipped > 0 {
            format!(", {} skipped", summary.skipped)
        } else {
            String::new()
        };
        println!("{} Test Summary: {}, {}{}", CHART, passed, failed, skipped);
    }

    /// Print step duration statistics table.
    pub fn print_stats_table(&self, results: &[(String, Duration, bool)], total: Duration) {
        println!();
        println!(
            "{} {:<16} {:<12} {}",
            CLOCK,
            style("Step").bold(),
            style("Duration").bold(),
            style("Status").bold()
        );
        for (name, dur, success) in results {
            let status = if *success {
                CHECK.to_string()
            } else {
                CROSS.to_string()
            };
            println!(
                "   {:<16} {:<12} {}",
                name,
                format_duration(*dur),
                status
            );
        }
        println!("{}", style("─".repeat(56)).dim());
        println!(
            "   {:<16} {}",
            "Total",
            style(format_duration(total)).bold()
        );
        println!();
    }
}

pub struct PipelineReporter;

impl PipelineReporter {
    pub fn new() -> Self {
        Self
    }

    /// Print pipeline header before execution
    pub fn print_header(&self, pipeline: &Pipeline) {
        println!();
        println!(
            "{} {} {}",
            ROCKET,
            style("Pipeline:").bold(),
            style(&pipeline.name).cyan().bold()
        );
        println!(
            "   {} steps, Docker-isolated execution",
            pipeline.steps.len()
        );
        println!("{}", style("─".repeat(60)).dim());
    }

    /// Print the result of a single step
    pub fn print_step_result(&self, result: &StepResult) {
        let status = if result.success {
            format!("{}{}", CHECK, style("PASS").green().bold())
        } else {
            format!("{}{}", CROSS, style("FAIL").red().bold())
        };

        println!();
        println!(
            "  {} {} [{}] {}{}",
            ARROW,
            style(&result.step_name).bold(),
            status,
            CLOCK,
            format_duration(result.duration)
        );

        // Print last few log lines for context (especially on failure)
        let lines_to_show = if result.success { 0 } else { 10 };
        if lines_to_show > 0 {
            let skip = result.logs.len().saturating_sub(lines_to_show);
            for line in result.logs.iter().skip(skip) {
                let prefix = match line.stream {
                    LogStream::Stdout => style("│ ").dim(),
                    LogStream::Stderr => style("│ ").red().dim(),
                };
                print!("    {}{}", prefix, line.message);
            }
        }
    }

    /// Print final summary
    pub fn print_summary(&self) {
        println!();
        println!("{}", style("─".repeat(60)).dim());
        println!(
            "  {} {}",
            CHECK,
            style("Pipeline completed").green().bold()
        );
        println!();
    }

    /// Print validation success
    pub fn print_validation_ok(&self, pipeline: &Pipeline) {
        println!(
            "{} Pipeline '{}' is valid ({} steps)",
            CHECK,
            style(&pipeline.name).cyan(),
            pipeline.steps.len()
        );
    }

    /// Print execution plan (dry-run)
    pub fn print_execution_plan(&self, pipeline: &Pipeline, scheduler: &Scheduler) {
        println!();
        println!(
            "{} {} {}",
            ROCKET,
            style("Execution plan for:").bold(),
            style(&pipeline.name).cyan().bold()
        );
        println!();

        let schedule = scheduler.resolve(None).unwrap_or_default();

        for (batch_idx, batch) in schedule.iter().enumerate() {
            let parallel_hint = if batch.len() > 1 {
                format!(" ({}parallel{})", style("").bold(), style("").bold())
            } else {
                String::new()
            };
            println!(
                "  {} Batch {}{}:",
                ARROW,
                style(batch_idx + 1).bold(),
                parallel_hint
            );
            for name in batch {
                let step = pipeline.get_step(name).unwrap();
                let deps = if step.depends_on.is_empty() {
                    String::new()
                } else {
                    format!(" ← [{}]", step.depends_on.join(", "))
                };
                println!(
                    "    {} {} ({}){}",
                    style("•").dim(),
                    style(name).bold(),
                    style(&step.image).dim(),
                    style(deps).dim()
                );
            }
        }
        println!();
    }

    /// Print step list
    pub fn print_step_list(&self, pipeline: &Pipeline) {
        println!(
            "\n{} Pipeline: {}\n",
            ARROW,
            style(&pipeline.name).cyan().bold()
        );
        for (i, step) in pipeline.steps.iter().enumerate() {
            let deps = if step.depends_on.is_empty() {
                String::new()
            } else {
                format!(" → depends on [{}]", step.depends_on.join(", "))
            };
            println!(
                "  {}. {} ({}){} ",
                style(i + 1).dim(),
                style(&step.name).bold(),
                style(&step.image).dim(),
                style(deps).dim()
            );
        }
        println!();
    }
}

pub fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
