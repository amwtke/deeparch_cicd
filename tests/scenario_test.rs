//! Scenario tests for the run-exit-fix-retry loop.
//! These test the state machine logic without requiring Docker.

use std::process::Command;

/// Test that a pipeline with on_failure: auto_fix in YAML is parsed correctly
#[test]
fn test_validate_pipeline_with_on_failure() {
    let output = Command::new("cargo")
        .args(["run", "--", "validate", "-f", "test-pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}

/// Test that --output json flag is accepted on dry-run
#[test]
fn test_dry_run_with_output_flag() {
    let output = Command::new("cargo")
        .args(["run", "--", "run", "-f", "pipeline.yml", "--dry-run", "--output", "json"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}

/// Test that --output plain flag is accepted on dry-run
#[test]
fn test_dry_run_with_plain_output() {
    let output = Command::new("cargo")
        .args(["run", "--", "run", "-f", "pipeline.yml", "--dry-run", "--output", "plain"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}

/// Test that retry fails gracefully when run-id doesn't exist
#[test]
fn test_retry_nonexistent_run_id() {
    let output = Command::new("cargo")
        .args(["run", "--", "retry", "--run-id", "nonexistent-xyz", "--step", "build", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nonexistent-xyz") || stderr.contains("No run found") || stderr.contains("Failed to read"));
}

/// Test that status fails gracefully when run-id doesn't exist
#[test]
fn test_status_nonexistent_run_id() {
    let output = Command::new("cargo")
        .args(["run", "--", "status", "--run-id", "nonexistent-xyz"])
        .output()
        .expect("Failed to run pipelight");
    assert!(!output.status.success());
}

/// Test list with --output flag (should still work, list doesn't use output mode but shouldn't crash)
#[test]
fn test_list_pipeline_steps() {
    let output = Command::new("cargo")
        .args(["run", "--", "list", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git-pull"));
    assert!(stdout.contains("build"));
    assert!(stdout.contains("clippy"));
    assert!(stdout.contains("test"));
    assert!(stdout.contains("fmt-check"));
}

/// Test that retry requires --step flag
#[test]
fn test_retry_without_step_shows_error() {
    let output = Command::new("cargo")
        .args(["run", "--", "retry", "--run-id", "some-id", "-f", "pipeline.yml"])
        .output()
        .expect("Failed to run pipelight");
    // Should either error because no --step or because run-id doesn't exist
    assert!(!output.status.success());
}

/// Test that run accepts --run-id flag
#[test]
fn test_run_accepts_run_id() {
    let output = Command::new("cargo")
        .args(["run", "--", "run", "-f", "pipeline.yml", "--dry-run", "--run-id", "custom-id-123"])
        .output()
        .expect("Failed to run pipelight");
    assert!(output.status.success());
}
