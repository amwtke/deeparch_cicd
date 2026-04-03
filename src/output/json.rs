use crate::run_state::RunState;

pub fn print_run_state(state: &RunState) {
    let json = serde_json::to_string_pretty(state).expect("Failed to serialize RunState");
    println!("{}", json);
}
