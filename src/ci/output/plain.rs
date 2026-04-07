use crate::ci::executor::{LogLine, LogStream};
use crate::run_state::{RunState, StepStatus};
use crate::ci::builder::test_parser::TestSummary;

/// Print step start
pub fn print_step_start(name: &str, image: &str) {
    println!("[{}] Starting... ({})", name, image);
}

/// Print a log line in plain mode
pub fn print_log_line(name: &str, line: &LogLine, verbose: bool) {
    if verbose {
        // In verbose mode, print all lines
        print!("[{}] {}", name, line.message);
    } else {
        // In non-verbose mode, only print stderr (error) lines
        if line.stream == LogStream::Stderr {
            print!("[{}] {}", name, line.message);
        }
    }
}

/// Print step completion
pub fn print_step_finish(name: &str, success: bool, duration: std::time::Duration) {
    let status = if success { "OK" } else { "FAIL" };
    println!("[{}] {} ({:.1}s)", name, status, duration.as_secs_f64());
}

/// Print test summary
pub fn print_test_summary(summary: &TestSummary) {
    println!();
    println!("Test Summary: {} passed, {} failed, {} skipped",
        summary.passed, summary.failed, summary.skipped);
}

/// Print step duration statistics table
pub fn print_stats_table(results: &[(String, std::time::Duration, bool)], total: std::time::Duration) {
    println!();
    println!("{:<16} {:<12} {}", "Step", "Duration", "Status");
    for (name, dur, success) in results {
        let status = if *success { "OK" } else { "FAIL" };
        println!("{:<16} {:<12} {}", name, format!("{:.1}s", dur.as_secs_f64()), status);
    }
    println!("{:<16} {:.1}s", "Total", total.as_secs_f64());
    println!();
}

/// Print final run state (existing function, keep for status/retry output)
pub fn print_run_state(state: &RunState) {
    println!("Pipeline: {} [{:?}]", state.pipeline, state.status);
    if let Some(ms) = state.duration_ms {
        println!("Duration: {:.1}s", ms as f64 / 1000.0);
    }
    println!();
    for step in &state.steps {
        let icon = match step.status {
            StepStatus::Success => "[OK]",
            StepStatus::Failed => "[FAIL]",
            StepStatus::Skipped => "[SKIP]",
            StepStatus::Running => "[..]",
            StepStatus::Pending => "[--]",
        };
        println!("  {} {} ({})", icon, step.name, step.image);
        if step.status == StepStatus::Failed {
            if let Some(ref stderr) = step.stderr {
                for line in stderr.lines().take(10) {
                    println!("    | {}", line);
                }
            }
        }
    }
}
