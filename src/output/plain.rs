use crate::run_state::{RunState, StepStatus};

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
