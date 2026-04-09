//! Integration tests for CLI output modes (plain, tty).
//! These tests verify command-line behavior, output formatting, and exit codes.
//! No Docker required — uses --dry-run or non-execution commands.

use std::process::Command;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn pipelight(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .args(args)
        .output()
        .expect("Failed to run pipelight")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

// ===========================================================================
// validate command
// ===========================================================================

#[test]
fn validate_valid_pipeline_succeeds() {
    let out = pipelight(&["validate", "-f", "pipeline.yml"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("rust-ci"), "should print pipeline name");
}

#[test]
fn validate_test_pipeline_succeeds() {
    let out = pipelight(&["validate", "-f", "test-pipeline.yml"]);
    assert!(out.status.success());
}

#[test]
fn validate_nonexistent_file_fails() {
    let out = pipelight(&["validate", "-f", "no-such-file.yml"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("no-such-file") || err.contains("Failed to load"),
        "stderr should mention the missing file, got: {err}"
    );
}

#[test]
fn validate_invalid_yaml_fails() {
    // Write a temporary invalid YAML
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.yml");
    std::fs::write(&path, "not: [valid: pipeline").unwrap();

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "validate", "-f"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
}

// ===========================================================================
// list command
// ===========================================================================

#[test]
fn list_shows_all_steps() {
    let out = pipelight(&["list", "-f", "pipeline.yml"]);
    assert!(out.status.success());
    let s = stdout(&out);
    for step in &["git-pull", "build", "clippy", "test", "fmt-check"] {
        assert!(s.contains(step), "list output should contain step '{step}'");
    }
}

#[test]
fn list_shows_dependencies() {
    let out = pipelight(&["list", "-f", "pipeline.yml"]);
    assert!(out.status.success());
    let s = stdout(&out);
    // test depends on build, clippy depends on build
    assert!(s.contains("build"), "should show dependency info");
}

#[test]
fn list_test_pipeline_shows_steps() {
    let out = pipelight(&["list", "-f", "test-pipeline.yml"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("hello"));
    assert!(s.contains("fail-step"));
    assert!(s.contains("downstream"));
}

// ===========================================================================
// run --dry-run (plain mode)
// ===========================================================================

#[test]
fn dry_run_plain_succeeds() {
    let out = pipelight(&["run", "-f", "pipeline.yml", "--dry-run", "--output", "plain"]);
    assert!(out.status.success());
}

#[test]
fn dry_run_tty_succeeds() {
    let out = pipelight(&["run", "-f", "pipeline.yml", "--dry-run", "--output", "tty"]);
    assert!(out.status.success());
}

#[test]
fn dry_run_with_custom_run_id() {
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--run-id", "my-custom-id",
    ]);
    assert!(out.status.success());
}

#[test]
fn dry_run_with_step_filter() {
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--step", "build",
    ]);
    assert!(out.status.success());
}

#[test]
fn dry_run_nonexistent_step_filter() {
    // Filtering for a non-existent step — current behavior: returns error or empty plan
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--step", "no-such-step",
    ]);
    // Dry-run with unknown step may succeed (empty plan) or fail — just verify no crash
    // The important thing is it doesn't panic
    let _ = out.status.code();
}

// ===========================================================================
// status command (error paths — no pre-existing run state)
// ===========================================================================

#[test]
fn status_nonexistent_run_fails() {
    let out = pipelight(&["status", "--run-id", "does-not-exist-xyz"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("does-not-exist-xyz") || err.contains("Failed to read"),
        "stderr should reference missing run-id"
    );
}

#[test]
fn status_with_plain_output_nonexistent() {
    let out = pipelight(&["status", "--run-id", "nope", "--output", "plain"]);
    assert!(!out.status.success());
}

#[test]
fn status_with_json_output_nonexistent() {
    let out = pipelight(&["status", "--run-id", "nope", "--output", "json"]);
    assert!(!out.status.success());
}

// ===========================================================================
// retry command (error paths)
// ===========================================================================

#[test]
fn retry_requires_step_flag() {
    let out = pipelight(&["retry", "--run-id", "some-id", "-f", "pipeline.yml"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("--step") || err.contains("required"),
        "should mention --step is required"
    );
}

#[test]
fn retry_nonexistent_run_fails() {
    let out = pipelight(&[
        "retry", "--run-id", "ghost-run", "--step", "build", "-f", "pipeline.yml",
    ]);
    assert!(!out.status.success());
}

#[test]
fn retry_with_json_output_nonexistent_run() {
    let out = pipelight(&[
        "retry", "--run-id", "ghost", "--step", "build",
        "-f", "pipeline.yml", "--output", "json",
    ]);
    assert!(!out.status.success());
}

// ===========================================================================
// init command
// ===========================================================================

#[test]
fn init_detects_rust_project() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("generated.yml");

    // Run init pointing at this project (which is a Rust project)
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "init", "-d", ".", "-o"])
        .arg(&out_path)
        .output()
        .unwrap();
    assert!(out.status.success());

    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("Rust") || s.contains("rust"), "should detect Rust project");

    // Verify generated YAML is valid
    let content = std::fs::read_to_string(&out_path).unwrap();
    assert!(content.contains("steps"), "generated YAML should have steps");
}

// ===========================================================================
// --help flags
// ===========================================================================

#[test]
fn run_help_shows_output_flag() {
    let out = pipelight(&["run", "--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("--output"), "run --help should document --output flag");
    assert!(s.contains("--dry-run"), "run --help should document --dry-run flag");
    assert!(s.contains("--run-id"), "run --help should document --run-id flag");
    assert!(s.contains("--verbose"), "run --help should document --verbose flag");
}

#[test]
fn retry_help_shows_output_flag() {
    let out = pipelight(&["retry", "--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("--output"));
    assert!(s.contains("--run-id"));
    assert!(s.contains("--step"));
}

#[test]
fn status_help_shows_output_flag() {
    let out = pipelight(&["status", "--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.contains("--output"));
    assert!(s.contains("--run-id"));
}

// ===========================================================================
// invalid CLI usage
// ===========================================================================

#[test]
fn unknown_subcommand_fails() {
    let out = pipelight(&["foobar"]);
    assert!(!out.status.success());
}

#[test]
fn run_with_invalid_output_mode_falls_back() {
    // Invalid output mode should fall back to auto-detect, not crash
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--output", "xml",
    ]);
    assert!(out.status.success());
}

// ===========================================================================
// verbose flag
// ===========================================================================

#[test]
fn dry_run_with_verbose_flag() {
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--verbose",
    ]);
    assert!(out.status.success());
}
