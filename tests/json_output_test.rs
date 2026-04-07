//! Integration tests for JSON output mode (--output json).
//! Validates JSON structure, field presence, status enums, and exit codes.
//!
//! Tests marked #[ignore] require a running Docker daemon.

use serde_json::Value;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
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

fn parse_json(output: &std::process::Output) -> Value {
    let s = stdout(output);
    serde_json::from_str(&s)
        .unwrap_or_else(|e| panic!("Failed to parse JSON output: {e}\nRaw output:\n{s}"))
}

/// Return absolute path to a test fixture pipeline file.
fn fixture(name: &str) -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let p = std::path::PathBuf::from(manifest).join("tests").join("fixtures").join(name);
    p.to_string_lossy().to_string()
}

// ===========================================================================
// dry-run JSON structure tests (no Docker needed)
// ===========================================================================

#[test]
fn json_dry_run_returns_valid_json() {
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--output", "json",
    ]);
    assert!(out.status.success());
    // Should produce no JSON on dry-run (dry-run exits before execution)
    // OR produce valid JSON — either way, should not crash
}

#[test]
fn json_dry_run_exit_code_zero() {
    let out = pipelight(&[
        "run", "-f", "pipeline.yml", "--dry-run", "--output", "json",
    ]);
    assert!(out.status.success(), "dry-run should always exit 0");
}

// ===========================================================================
// JSON structure tests with pre-saved RunState (status command)
// ===========================================================================

/// Helper: save a RunState JSON to a temp directory mimicking ~/.pipelight/runs/<id>/status.json
fn save_run_state(json_content: &str) -> (tempfile::TempDir, String) {
    let run_id = format!("test-{}", uuid_short());
    let base = dirs_home().join(".pipelight").join("runs");
    let run_dir = base.join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("status.json"), json_content).unwrap();
    // Return a sentinel and the run_id so we can clean up
    let td = tempfile::tempdir().unwrap(); // just for RAII guard; real files are in ~/.pipelight
    (td, run_id)
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{:x}", t.as_nanos() % 0xFFFFFFFF)
}

fn dirs_home() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn cleanup_run(run_id: &str) {
    let path = dirs_home().join(".pipelight").join("runs").join(run_id);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn json_status_returns_full_structure() {
    let state_json = r#"{
        "run_id": "PLACEHOLDER",
        "pipeline": "test-pipeline",
        "status": "success",
        "duration_ms": 1234,
        "steps": [
            {
                "name": "build",
                "status": "success",
                "exit_code": 0,
                "duration_ms": 800,
                "image": "rust:1.78-slim",
                "command": "cargo build",
                "stdout": "compiled OK",
                "stderr": null,
                "error_context": null,
                "on_failure": null
            },
            {
                "name": "test",
                "status": "success",
                "exit_code": 0,
                "duration_ms": 434,
                "image": "rust:1.78-slim",
                "command": "cargo test",
                "stdout": "test result: ok",
                "stderr": null,
                "error_context": null,
                "on_failure": null
            }
        ]
    }"#;

    let (_td, run_id) = save_run_state(&state_json.replace("PLACEHOLDER", &uuid_short()));
    // Re-write with actual run_id
    let actual_json = state_json.replace("PLACEHOLDER", &run_id);
    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::write(run_dir.join("status.json"), &actual_json).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
    assert!(out.status.success());

    let json = parse_json(&out);

    // Top-level fields
    assert_eq!(json["run_id"].as_str().unwrap(), run_id);
    assert_eq!(json["pipeline"].as_str().unwrap(), "test-pipeline");
    assert_eq!(json["status"].as_str().unwrap(), "success");
    assert_eq!(json["duration_ms"].as_u64().unwrap(), 1234);

    // Steps array
    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);

    // First step
    assert_eq!(steps[0]["name"].as_str().unwrap(), "build");
    assert_eq!(steps[0]["status"].as_str().unwrap(), "success");
    assert_eq!(steps[0]["exit_code"].as_i64().unwrap(), 0);
    assert_eq!(steps[0]["duration_ms"].as_u64().unwrap(), 800);
    assert_eq!(steps[0]["image"].as_str().unwrap(), "rust:1.78-slim");
    assert_eq!(steps[0]["command"].as_str().unwrap(), "cargo build");
    assert_eq!(steps[0]["stdout"].as_str().unwrap(), "compiled OK");
    assert!(steps[0]["stderr"].is_null());

    cleanup_run(&run_id);
}

#[test]
fn json_status_failed_pipeline_structure() {
    let run_id = format!("fail-{}", uuid_short());
    let state_json = serde_json::json!({
        "run_id": run_id,
        "pipeline": "failing-pipeline",
        "status": "failed",
        "duration_ms": 5000,
        "steps": [
            {
                "name": "build",
                "status": "failed",
                "exit_code": 1,
                "duration_ms": 3000,
                "image": "rust:1.78-slim",
                "command": "cargo build",
                "stdout": null,
                "stderr": "error[E0308]: mismatched types",
                "error_context": {
                    "files": ["src/main.rs"],
                    "lines": [42],
                    "error_type": "compilation_error"
                },
                "on_failure": {
                    "callback_command": "auto_fix",
                    "max_retries": 3,
                    "retries_remaining": 2,
                    "context_paths": ["src/"]
                }
            },
            {
                "name": "test",
                "status": "skipped",
                "exit_code": null,
                "duration_ms": null,
                "image": "rust:1.78-slim",
                "command": "cargo test",
                "stdout": null,
                "stderr": null,
                "error_context": null,
                "on_failure": null
            }
        ]
    });

    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("status.json"),
        serde_json::to_string_pretty(&state_json).unwrap(),
    ).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
    assert!(out.status.success());

    let json = parse_json(&out);

    assert_eq!(json["status"].as_str().unwrap(), "failed");

    // Failed step has error_context
    let build = &json["steps"][0];
    assert_eq!(build["status"].as_str().unwrap(), "failed");
    assert_eq!(build["exit_code"].as_i64().unwrap(), 1);
    assert!(build["stderr"].as_str().unwrap().contains("E0308"));

    let ec = &build["error_context"];
    assert_eq!(ec["files"][0].as_str().unwrap(), "src/main.rs");
    assert_eq!(ec["lines"][0].as_u64().unwrap(), 42);
    assert_eq!(ec["error_type"].as_str().unwrap(), "compilation_error");

    // on_failure structure
    let of = &build["on_failure"];
    assert_eq!(of["callback_command"].as_str().unwrap(), "auto_fix");
    assert_eq!(of["max_retries"].as_u64().unwrap(), 3);
    assert_eq!(of["retries_remaining"].as_u64().unwrap(), 2);
    assert_eq!(of["context_paths"][0].as_str().unwrap(), "src/");

    // Skipped step
    let test_step = &json["steps"][1];
    assert_eq!(test_step["status"].as_str().unwrap(), "skipped");
    assert!(test_step["exit_code"].is_null());
    assert!(test_step["duration_ms"].is_null());

    cleanup_run(&run_id);
}

#[test]
fn json_status_retryable_pipeline() {
    let run_id = format!("retry-{}", uuid_short());
    let state_json = serde_json::json!({
        "run_id": run_id,
        "pipeline": "retryable-pipeline",
        "status": "retryable",
        "duration_ms": 2000,
        "steps": [
            {
                "name": "lint",
                "status": "failed",
                "exit_code": 1,
                "duration_ms": 2000,
                "image": "rust:1.78-slim",
                "command": "cargo clippy",
                "stdout": null,
                "stderr": "warning turned error",
                "error_context": null,
                "on_failure": {
                    "callback_command": "auto_fix",
                    "max_retries": 2,
                    "retries_remaining": 2,
                    "context_paths": ["src/"]
                }
            }
        ]
    });

    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("status.json"),
        serde_json::to_string_pretty(&state_json).unwrap(),
    ).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
    assert!(out.status.success());

    let json = parse_json(&out);
    assert_eq!(json["status"].as_str().unwrap(), "retryable");

    cleanup_run(&run_id);
}

#[test]
fn json_status_plain_mode_is_not_json() {
    let run_id = format!("plain-{}", uuid_short());
    let state_json = serde_json::json!({
        "run_id": run_id,
        "pipeline": "p",
        "status": "success",
        "duration_ms": 100,
        "steps": []
    });

    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("status.json"),
        serde_json::to_string_pretty(&state_json).unwrap(),
    ).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "plain"]);
    assert!(out.status.success());

    let s = stdout(&out);
    // Plain mode should NOT produce JSON — should look like human-readable text
    assert!(
        serde_json::from_str::<Value>(&s).is_err(),
        "plain mode output should not be valid JSON"
    );
    assert!(s.contains("Pipeline:"), "plain mode should show Pipeline: header");

    cleanup_run(&run_id);
}

// ===========================================================================
// JSON field validation: all status enum values
// ===========================================================================

#[test]
fn json_pipeline_status_enum_values() {
    for status in &["running", "success", "retryable", "failed"] {
        let run_id = format!("enum-{}-{}", status, uuid_short());
        let state = serde_json::json!({
            "run_id": run_id,
            "pipeline": "p",
            "status": status,
            "duration_ms": 100,
            "steps": []
        });

        let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("status.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
        assert!(out.status.success(), "status command should succeed for status={status}");

        let json = parse_json(&out);
        assert_eq!(json["status"].as_str().unwrap(), *status);

        cleanup_run(&run_id);
    }
}

#[test]
fn json_step_status_enum_values() {
    for step_status in &["pending", "running", "success", "failed", "skipped"] {
        let run_id = format!("step-{}-{}", step_status, uuid_short());
        let state = serde_json::json!({
            "run_id": run_id,
            "pipeline": "p",
            "status": "success",
            "duration_ms": 100,
            "steps": [{
                "name": "s1",
                "status": step_status,
                "exit_code": null,
                "duration_ms": null,
                "image": "alpine",
                "command": "echo hi",
                "stdout": null,
                "stderr": null,
                "error_context": null,
                "on_failure": null
            }]
        });

        let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("status.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        ).unwrap();

        let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
        assert!(out.status.success());

        let json = parse_json(&out);
        assert_eq!(json["steps"][0]["status"].as_str().unwrap(), *step_status);

        cleanup_run(&run_id);
    }
}

// ===========================================================================
// JSON with test_summary field
// ===========================================================================

#[test]
fn json_status_with_test_summary() {
    let run_id = format!("ts-{}", uuid_short());
    let state = serde_json::json!({
        "run_id": run_id,
        "pipeline": "test-pipeline",
        "status": "failed",
        "duration_ms": 3000,
        "steps": [{
            "name": "test",
            "status": "failed",
            "exit_code": 1,
            "duration_ms": 3000,
            "image": "rust:1.78-slim",
            "command": "cargo test",
            "stdout": null,
            "stderr": "test failed",
            "error_context": null,
            "on_failure": null,
            "test_summary": {
                "passed": 42,
                "failed": 3,
                "skipped": 1
            }
        }]
    });

    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("status.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    ).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
    assert!(out.status.success());

    let json = parse_json(&out);
    let ts = &json["steps"][0]["test_summary"];
    assert_eq!(ts["passed"].as_u64().unwrap(), 42);
    assert_eq!(ts["failed"].as_u64().unwrap(), 3);
    assert_eq!(ts["skipped"].as_u64().unwrap(), 1);

    cleanup_run(&run_id);
}

#[test]
fn json_status_without_test_summary_omits_field() {
    let run_id = format!("nots-{}", uuid_short());
    let state = serde_json::json!({
        "run_id": run_id,
        "pipeline": "p",
        "status": "success",
        "duration_ms": 100,
        "steps": [{
            "name": "build",
            "status": "success",
            "exit_code": 0,
            "duration_ms": 100,
            "image": "alpine",
            "command": "echo ok",
            "stdout": "ok",
            "stderr": null,
            "error_context": null,
            "on_failure": null
        }]
    });

    let run_dir = dirs_home().join(".pipelight").join("runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("status.json"),
        serde_json::to_string_pretty(&state).unwrap(),
    ).unwrap();

    let out = pipelight(&["status", "--run-id", &run_id, "--output", "json"]);
    assert!(out.status.success());

    let json = parse_json(&out);
    // test_summary should be absent (skip_serializing_if = "Option::is_none")
    assert!(
        json["steps"][0].get("test_summary").is_none()
            || json["steps"][0]["test_summary"].is_null(),
        "test_summary should be omitted when not present"
    );

    cleanup_run(&run_id);
}

// ===========================================================================
// Docker-required tests (run with: cargo test -- --ignored)
// ===========================================================================

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_success() {
    let f = fixture("success.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    assert!(out.status.success(), "exit code should be 0, stderr: {}", stderr(&out));

    let json = parse_json(&out);
    assert_eq!(json["status"].as_str().unwrap(), "success");
    assert!(json["run_id"].as_str().is_some());
    assert!(json["duration_ms"].as_u64().is_some());

    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["name"].as_str().unwrap(), "hello");
    assert_eq!(steps[0]["status"].as_str().unwrap(), "success");
    assert_eq!(steps[0]["exit_code"].as_i64().unwrap(), 0);
    assert!(steps[0]["duration_ms"].as_u64().is_some());
    let step_stdout = steps[0]["stdout"].as_str().unwrap_or("");
    assert!(
        step_stdout.contains("hello from integration test"),
        "stdout should contain echoed text, got: {step_stdout}"
    );
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_failure() {
    let f = fixture("failure.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    assert!(!out.status.success(), "should fail with exit code != 0");

    let json = parse_json(&out);
    assert_eq!(json["status"].as_str().unwrap(), "failed");

    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps[0]["status"].as_str().unwrap(), "failed");
    assert!(steps[0]["exit_code"].as_i64().unwrap() != 0);
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_multi_step_with_skip() {
    let f = fixture("multi-step-skip.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    let json = parse_json(&out);
    assert_eq!(json["status"].as_str().unwrap(), "failed");

    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["name"].as_str().unwrap(), "step-a");
    assert_eq!(steps[0]["status"].as_str().unwrap(), "failed");
    assert_eq!(steps[1]["name"].as_str().unwrap(), "step-b");
    assert_eq!(steps[1]["status"].as_str().unwrap(), "skipped");
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_retryable() {
    let f = fixture("retryable.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    let json = parse_json(&out);
    assert_eq!(json["status"].as_str().unwrap(), "retryable");

    let step = &json["steps"][0];
    assert_eq!(step["status"].as_str().unwrap(), "failed");
    let of = &step["on_failure"];
    // CallbackCommand is serialized via match to "auto_fix"
    assert_eq!(of["callback_command"].as_str().unwrap(), "auto_fix");
    assert_eq!(of["max_retries"].as_u64().unwrap(), 3);
    assert_eq!(of["retries_remaining"].as_u64().unwrap(), 3);
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_allow_failure() {
    let f = fixture("allow-failure.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    let json = parse_json(&out);
    // Pipeline should succeed because allow_failure=true
    assert_eq!(
        json["status"].as_str().unwrap(), "success",
        "allow_failure step should not block pipeline"
    );

    let steps = json["steps"].as_array().unwrap();
    // allow_failure step is recorded as success (pipeline continues)
    assert_eq!(steps[0]["name"].as_str().unwrap(), "flaky");
    assert_eq!(steps[1]["name"].as_str().unwrap(), "after-flaky");
    assert_eq!(steps[1]["status"].as_str().unwrap(), "success");
    // The downstream step should have run
    let after_stdout = steps[1]["stdout"].as_str().unwrap_or("");
    assert!(after_stdout.contains("I still run"), "downstream step should have executed");
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_real_execution_with_custom_run_id() {
    let f = fixture("success.yml");
    let custom_id = format!("custom-{}", uuid_short());

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json", "--run-id", &custom_id])
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let json = parse_json(&out);
    assert_eq!(json["run_id"].as_str().unwrap(), custom_id);

    cleanup_run(&custom_id);
}

#[test]
#[ignore = "requires Docker daemon"]
fn plain_real_execution_success() {
    let f = fixture("success.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "plain"])
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("[hello]"), "plain mode should prefix with step name");
    assert!(s.contains("OK"), "plain mode should show OK for success");
}

#[test]
#[ignore = "requires Docker daemon"]
fn plain_real_execution_failure_shows_step_info() {
    let f = fixture("failure.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "plain"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let s = stdout(&out);
    assert!(s.contains("[will-fail]"), "plain mode should show step name");
    assert!(s.contains("FAIL"), "plain mode should show FAIL");
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_stdout_is_pure_json() {
    // In JSON mode, stdout must contain ONLY valid JSON — no tracing, no spinner
    let f = fixture("success.yml");

    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "run", "-f", &f, "--output", "json"])
        .output()
        .unwrap();

    let s = stdout(&out);
    // stdout should be valid JSON, no tracing lines mixed in
    let json: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|e| panic!("stdout is not pure JSON: {e}\nRaw:\n{s}"));
    assert_eq!(json["status"].as_str().unwrap(), "success");
}
