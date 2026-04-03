use console::{style, Emoji};

use crate::executor::{LogStream, StepResult};
use crate::pipeline::Pipeline;
use crate::scheduler::Scheduler;

static ROCKET: Emoji<'_, '_> = Emoji("🚀 ", ">> ");
static CHECK: Emoji<'_, '_> = Emoji("✅ ", "[OK] ");
static CROSS: Emoji<'_, '_> = Emoji("❌ ", "[FAIL] ");
static ARROW: Emoji<'_, '_> = Emoji("▸ ", "> ");
static CLOCK: Emoji<'_, '_> = Emoji("⏱  ", "");

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

        let depths = scheduler.step_depths();
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

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
