# Architecture

## Current Layers

```
CLI (clap)               → subcommands: run / validate / list / logs
    ↓
Pipeline Model (serde)   → YAML → DAG parse, variable interpolation, conditions
    ↓
Scheduler (petgraph)     → DAG topological sort, parallel step scheduling (tokio)
    ↓
Executor (bollard)       → Docker container lifecycle management
    ↓
Output (console)         → Real-time colored terminal output
```

## Implemented: Run-Exit-Fix-Retry Loop

```
pipelight run --output json
    → execute steps in DAG order
    → step fails (strategy: auto_fix)
    → save state to ~/.pipelight/runs/<run-id>/status.json
    → output JSON with error details + on_failure metadata
    → exit (code 1 = retryable)

LLM agent reads JSON, fixes code...

pipelight retry --run-id <id> --step <name> --output json
    → load state from status.json
    → re-execute failed step
    → if success: run downstream steps
    → output updated JSON
    → exit (code 0 = success, 1 = still retryable, 2 = final failure)
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All steps succeeded |
| 1 | Step failed, retryable (auto_fix with retries remaining) |
| 2 | Step failed, final (abort, notify, or max_retries exhausted) |

## Future: LLM Integration

### MCP Server (planned)

Expose pipelight as MCP tools to Claude Code:

- `pipelight.run` — trigger a pipeline
- `pipelight.status` — check pipeline status  
- `pipelight.logs` — get step logs

## Directory Structure

```
src/
  main.rs              → entry point, exit code handling
  cli/mod.rs           → clap commands: run, validate, list, retry, status
  pipeline/mod.rs      → YAML parsing, Pipeline/Step/OnFailure/Strategy model
  scheduler/mod.rs     → DAG construction & topological sort scheduling
  executor/mod.rs      → Docker container executor (bollard)
  run_state/mod.rs     → RunState/StepState model, status.json persistence
  output/
    mod.rs             → OutputMode enum (Tty/Plain/Json), auto-detection
    tty.rs             → colored terminal output with progress
    json.rs            → structured JSON output for LLM agents
    plain.rs           → plain text output (no ANSI codes)
tests/
  integration_test.rs  → CLI integration tests
```

## Key Crates

| Crate | Purpose |
|-------|---------|
| clap | CLI argument parsing |
| tokio | async runtime |
| serde + serde_yaml | YAML config parsing |
| bollard | Docker API (no shelling out to docker CLI) |
| petgraph | DAG construction and scheduling |
| anyhow + thiserror | error handling |
| tracing | structured logging |
| futures-util | stream utilities for Docker log streaming |
