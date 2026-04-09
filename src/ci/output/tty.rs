use console::{style, Emoji, Term};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::ci::executor::{LogLine, LogStream, StepResult};
use crate::ci::parser::Pipeline;
use crate::ci::pipeline_builder::test_parser::TestSummary;
use crate::ci::scheduler::Scheduler;

static ROCKET: Emoji<'_, '_> = Emoji("🚀 ", ">> ");
static CHECK: Emoji<'_, '_> = Emoji("✅", "[OK]");
static CROSS: Emoji<'_, '_> = Emoji("❌", "[FAIL]");
static ARROW: Emoji<'_, '_> = Emoji("▸ ", "> ");
static CLOCK: Emoji<'_, '_> = Emoji("⏱  ", "");
static PENDING: Emoji<'_, '_> = Emoji("⬚", "[ ]");
static CHART: Emoji<'_, '_> = Emoji("📊 ", "");
static RUNNING: Emoji<'_, '_> = Emoji("⏳", "[..]");

/// Two-line progress UI for TTY mode.
///
/// Line 1 (static, reprinted): pipeline flow showing all steps with status icons
///   🚀 maven-java-ci: ✅build → ⏳test → ⬚package
///
/// Line 2 (spinner): current step with latest log line
///   ⠹ [2/3] test 45.3s | [INFO] Running com.example.UserTest
pub struct PipelineProgressUI {
    step_names: Vec<String>,
    step_status: Vec<StepStage>,
    current_index: usize,
    spinner: Option<ProgressBar>,
    pipeline_name: String,
    last_log: String,
    verbose: bool,
}

#[derive(Clone, PartialEq)]
enum StepStage {
    Pending,
    Running,
    Success,
    Failed,
}

impl PipelineProgressUI {
    pub fn new(step_names: &[String], verbose: bool) -> Self {
        let step_status = vec![StepStage::Pending; step_names.len()];
        Self {
            step_names: step_names.to_vec(),
            step_status,
            current_index: 0,
            spinner: None,
            pipeline_name: String::new(),
            last_log: String::new(),
            verbose,
        }
    }

    /// Print initial header.
    pub fn print_header(&mut self, pipeline_name: &str, _step_count: usize) {
        self.pipeline_name = pipeline_name.to_string();
        println!();
        // Print the pipeline flow line (will be reprinted on updates)
        println!("{}", self.format_flow_line());
    }

    /// Build the flow line: 🚀 maven-java-ci: ✅build → ⏳test → ⬚package
    fn format_flow_line(&self) -> String {
        let mut parts = Vec::new();
        for (i, name) in self.step_names.iter().enumerate() {
            let icon = match self.step_status[i] {
                StepStage::Pending => PENDING.to_string(),
                StepStage::Running => RUNNING.to_string(),
                StepStage::Success => CHECK.to_string(),
                StepStage::Failed => CROSS.to_string(),
            };
            let name_styled = match self.step_status[i] {
                StepStage::Pending => style(name).dim().to_string(),
                StepStage::Running => style(name).yellow().bold().to_string(),
                StepStage::Success => style(name).green().to_string(),
                StepStage::Failed => style(name).red().to_string(),
            };
            parts.push(format!("{}{}", icon, name_styled));
        }
        format!(
            "{} {}: {}",
            ROCKET,
            style(&self.pipeline_name).cyan().bold(),
            parts.join(&format!(" {} ", style("→").dim()))
        )
    }

    /// Reprint the flow line by moving cursor up and overwriting.
    fn refresh_flow_line(&self) {
        let term = Term::stderr();
        // Move up 2 lines (flow line + spinner line), clear, reprint flow
        let _ = term.move_cursor_up(1);
        let _ = term.clear_line();
        // We use the spinner's suspend to print above it
        if let Some(ref pb) = self.spinner {
            pb.suspend(|| {
                let term = Term::stderr();
                let _ = term.move_cursor_up(1);
                let _ = term.clear_line();
                println!("{}", self.format_flow_line());
            });
        }
    }

    /// Mark a step as started.
    pub fn start_step(&mut self, name: &str) {
        // Find step index
        if let Some(idx) = self.step_names.iter().position(|n| n == name) {
            self.step_status[idx] = StepStage::Running;
            self.current_index = idx + 1;
        }

        self.last_log.clear();

        // Stop previous spinner
        if let Some(pb) = self.spinner.take() {
            pb.finish_and_clear();
        }

        // Reprint flow line with updated status
        // Clear previous flow line and print new one
        let term = Term::stderr();
        let _ = term.move_cursor_up(1);
        let _ = term.clear_line();
        println!("{}", self.format_flow_line());

        // Start new spinner
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner} {msg}")
                .unwrap(),
        );
        let progress_tag = format!("[{}/{}]", self.current_index, self.step_names.len());
        pb.set_message(format!(
            "{} {} {}",
            style(&progress_tag).dim(),
            style(name).bold(),
            style("Running...").dim()
        ));
        pb.enable_steady_tick(Duration::from_millis(100));
        self.spinner = Some(pb);
    }

    /// Update spinner with latest log line.
    pub fn update_log(&mut self, name: &str, line: &LogLine) {
        let msg = line.message.trim_end();
        if msg.is_empty() {
            return;
        }
        self.last_log = msg.to_string();

        // Truncate log to fit terminal (char-safe for multi-byte UTF-8)
        let max_log_chars = 60;
        let log_display = if msg.chars().count() > max_log_chars {
            let truncated: String = msg.chars().take(max_log_chars).collect();
            format!("{}...", truncated)
        } else {
            msg.to_string()
        };

        if let Some(ref pb) = self.spinner {
            let progress_tag = format!("[{}/{}]", self.current_index, self.step_names.len());
            pb.set_message(format!(
                "{} {} {} {}",
                style(&progress_tag).dim(),
                style(name).bold(),
                style("|").dim(),
                style(&log_display).dim()
            ));
        }

        // In verbose mode, also print full log lines above spinner
        if self.verbose {
            if let Some(ref pb) = self.spinner {
                let prefix = match line.stream {
                    LogStream::Stdout => style("   │ ").dim(),
                    LogStream::Stderr => style("   │ ").red().dim(),
                };
                pb.suspend(|| {
                    // Reprint flow line first since suspend clears spinner area
                    print!("{}{}", prefix, line.message);
                });
            }
        }
    }

    /// Mark step as finished with report summary and log path.
    pub fn finish_step(&mut self, name: &str, success: bool, duration: Duration) {
        self.finish_step_with_report(name, success, duration, None, None);
    }

    /// Mark step as finished with optional report info.
    pub fn finish_step_with_report(
        &mut self,
        name: &str,
        success: bool,
        duration: Duration,
        report_summary: Option<&str>,
        report_path: Option<&str>,
    ) {
        // Update status
        if let Some(idx) = self.step_names.iter().position(|n| n == name) {
            self.step_status[idx] = if success {
                StepStage::Success
            } else {
                StepStage::Failed
            };
        }

        // Clear spinner
        if let Some(pb) = self.spinner.take() {
            pb.finish_and_clear();
        }

        // Reprint flow line with updated status
        let term = Term::stderr();
        let _ = term.move_cursor_up(1);
        let _ = term.clear_line();
        println!("{}", self.format_flow_line());

        // Print completion line
        let progress_tag = format!("[{}/{}]", self.current_index, self.step_names.len());
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

        let report_str = match (report_summary, report_path) {
            (Some(summary), Some(path)) => format!(
                "  {} → {}",
                style(summary).italic(),
                style(path).dim().underlined()
            ),
            (Some(summary), None) => format!("  {}", style(summary).italic()),
            (None, Some(path)) => format!("  → {}", style(path).dim().underlined()),
            (None, None) => String::new(),
        };

        println!(
            " {} {} {:<16} {}{}",
            icon,
            style(&progress_tag).dim(),
            name_styled,
            style(format_duration(duration)).dim(),
            report_str
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
            println!("   {:<16} {:<12} {}", name, format_duration(*dur), status);
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
                " (parallel)".to_string()
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
