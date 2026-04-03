use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(["run", "--", "--help"])
        .output()
        .expect("Failed to run pipelight");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run"));
    assert!(stdout.contains("retry"));
    assert!(stdout.contains("status"));
}

#[test]
fn test_validate_pipeline() {
    let output = Command::new("cargo")
        .args(["run", "--", "validate", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}

#[test]
fn test_list_pipeline() {
    let output = Command::new("cargo")
        .args(["run", "--", "list", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("build"));
}

#[test]
fn test_retry_help() {
    let output = Command::new("cargo")
        .args(["run", "--", "retry", "--help"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--run-id"));
    assert!(stdout.contains("--step"));
}

#[test]
fn test_status_help() {
    let output = Command::new("cargo")
        .args(["run", "--", "status", "--help"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--run-id"));
}
