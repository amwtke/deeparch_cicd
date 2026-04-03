# AI-Native CI/CD Design Spec

## Overview

Pipelight is a lightweight CLI CI/CD tool that is designed to work with any LLM agent. Unlike traditional CI/CD that mechanically runs commands and stops on failure, pipelight introduces a **pause-fix-retry loop** — when a step fails, the pipeline pauses and outputs structured data for an external LLM agent (or human) to analyze, fix, and retry.

Pipelight itself does NOT embed any LLM. It is LLM-agnostic by design.

## Design Principles

1. **LLM-agnostic** — no dependency on any specific LLM vendor. Pipelight outputs structured JSON; any agent (Claude Code, Cursor, Copilot, custom scripts) can consume it.
2. **CLI is the interface** — LLM agents call pipelight via shell commands, the most universal integration method.
3. **Self-describing pipelines** — `on_failure` metadata in pipeline.yml tells agents what to do when steps fail, without pipelight executing the fix itself.
4. **Backward compatible** — existing pipeline.yml files without `on_failure` work exactly as before (fail = abort).

## Core Innovation: Run-Exit-Fix-Retry Loop

```
Traditional CI/CD:
  run → fail → STOP → human reads log → human fixes → human re-triggers entire pipeline

Pipelight:
  run → fail → EXIT with JSON → agent reads JSON → agent fixes code → retry → run from breakpoint
                                  ↑__________________________________________________↓
                                              automatic fix loop
```

This loop is pipelight's key differentiator. The pipeline is not a one-way street — failure is the starting point for agent intervention.

**Critical design decision: run-and-exit, not long-running process.**

LLM agents (Claude Code, Cursor, Copilot) call shell commands and wait for them to finish. A long-running process that pauses and waits for a signal file would **deadlock** the agent — the agent can't read stdout until the process exits, so it can never know it needs to fix something.

Instead, every pipelight invocation is a complete run-and-exit cycle:
1. `pipelight run` — execute steps, output JSON, **exit**
2. Agent reads JSON, fixes code
3. `pipelight retry` — resume from breakpoint, output JSON, **exit**
4. Repeat until success or max_retries exhausted

Pipeline state is persisted in `~/.pipelight/runs/<run-id>/status.json` between invocations.

## Output Modes

Three output modes, selected via `--output` flag or auto-detected:

| Mode | Flag | When | Description |
|------|------|------|-------------|
| TTY | `--output tty` | stdout is terminal (default) | Colored progress bars, real-time log streaming |
| Plain | `--output plain` | stdout is not terminal (auto) | Plain text, no ANSI escape codes |
| JSON | `--output json` | explicit flag | Structured JSON, one complete object per pipeline run |

Auto-detection: if stdout is a TTY, default to `tty`; otherwise default to `plain`. `--output` flag always overrides auto-detection.

## JSON Output Structure

A complete pipeline run result:

```json
{
  "run_id": "abc123",
  "pipeline": "rust-ci",
  "status": "paused",
  "duration_ms": 12340,
  "steps": [
    {
      "name": "build",
      "status": "failed",
      "exit_code": 101,
      "duration_ms": 8200,
      "image": "rust:1.78-slim",
      "command": "cargo build --release",
      "stdout": "...",
      "stderr": "error[E0277]: the trait bound...",
      "error_context": {
        "files": ["src/executor/mod.rs"],
        "lines": [93],
        "error_type": "compile_error"
      },
      "on_failure": {
        "strategy": "auto_fix",
        "max_retries": 3,
        "retries_remaining": 3,
        "context_paths": ["src/", "Cargo.toml"]
      }
    },
    {
      "name": "lint",
      "status": "success",
      "exit_code": 0,
      "duration_ms": 4100,
      "image": "rust:1.78-slim",
      "command": "cargo clippy -- -D warnings",
      "stdout": "...",
      "stderr": ""
    },
    {
      "name": "test",
      "status": "skipped",
      "reason": "dependency 'build' failed"
    }
  ]
}
```

### error_context

Pipelight attempts to parse error output from known tools (rustc, gcc, pytest, etc.) to extract:
- `files` — source files involved in the error
- `lines` — line numbers
- `error_type` — categorization (compile_error, test_failure, lint_error, runtime_error, unknown)

If parsing fails, `error_context` is `null`. This is best-effort, not guaranteed.

### on_failure

Passed through from pipeline.yml configuration. Pipelight does not act on `strategy` itself — it only controls whether the pipeline exits with retryable state (auto_fix) or aborts permanently (abort/notify).

## Pipeline YAML Extension

### on_failure block

Added to step definitions. Entirely optional — omitting it defaults to `strategy: abort`.

```yaml
steps:
  - name: build
    image: rust:1.78-slim
    commands:
      - cargo build --release
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml
```

### strategy values

| Strategy | Meaning | Pipelight behavior |
|----------|---------|-------------------|
| `abort` | Fail and stop (default) | Mark step failed, skip downstream steps, exit |
| `auto_fix` | Suggest agent attempt fix | Save retryable state to status.json, exit with JSON output |
| `notify` | Inform but don't suggest fix | Mark step failed, include full info in JSON, abort and exit |

### Full example

```yaml
name: rust-ci

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: "1"

steps:
  - name: build
    image: rust:1.78-slim
    commands:
      - cargo build --release
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml

  - name: lint
    image: rust:1.78-slim
    commands:
      - rustup component add clippy
      - cargo clippy -- -D warnings
    on_failure:
      strategy: auto_fix
      max_retries: 2
      context_paths:
        - src/

  - name: test
    image: rust:1.78-slim
    depends_on: [build]
    commands:
      - cargo test --release
    on_failure:
      strategy: notify

  - name: fmt-check
    image: rust:1.78-slim
    commands:
      - rustup component add rustfmt
      - cargo fmt -- --check
    on_failure:
      strategy: auto_fix
      max_retries: 1
      context_paths:
        - src/

  - name: security-audit
    image: rust:1.78-slim
    depends_on: [build]
    allow_failure: true
    commands:
      - cargo install cargo-audit
      - cargo audit
```

## Retry Mechanism (Run-and-Exit)

### How it works

Every invocation is a complete run-and-exit cycle. No long-running processes, no signal files.

When a step with `strategy: auto_fix` fails:

1. Pipelight saves pipeline state to `~/.pipelight/runs/<run-id>/status.json`
2. Pipelight outputs JSON to stdout (including error details and on_failure metadata)
3. Pipelight **exits** (exit code 1 = retryable failure)
4. External agent (or human) reads JSON output, fixes the issue
5. Agent runs `pipelight retry --run-id <id> --step <name> --output json`
6. Pipelight starts a **new process**, reads status.json, re-executes the failed step
7. If step passes → continue running downstream steps → output JSON → exit
8. If step fails again → update status.json (decrement retries_remaining) → output JSON → exit
9. Agent repeats 4-8 until success or retries_remaining reaches 0
10. retries_remaining = 0 → output JSON with `"final": true` → exit

### State persistence via status.json

```
~/.pipelight/runs/<run-id>/
  status.json       ← complete pipeline state, persisted between invocations
```

status.json contains:
- Which steps succeeded, failed, or were skipped
- Retry counts remaining for each auto_fix step
- The original pipeline configuration
- Timestamps and duration for each step

This file is the **single source of truth** for pipeline state across multiple invocations.

- `pipelight run` creates status.json and writes initial state
- `pipelight retry` reads status.json, re-executes the specified step, updates status.json
- `pipelight status` reads and displays status.json

### Exit codes

| Exit code | Meaning |
|-----------|---------|
| 0 | All steps succeeded |
| 1 | Step failed, retryable (auto_fix with retries remaining) |
| 2 | Step failed, final (abort, notify, or max_retries exhausted) |

LLM agents can use exit codes for quick decision-making without parsing JSON.

### Run lifecycle

```
pipelight run (process 1)
    ├─ create ~/.pipelight/runs/<run-id>/status.json
    ├─ execute steps in DAG order
    ├─ step "build" fails (strategy: auto_fix)
    ├─ save state to status.json
    ├─ output JSON to stdout
    └─ exit (code 1)

    agent reads JSON, fixes code...

pipelight retry --run-id <id> --step build (process 2)
    ├─ read status.json, restore pipeline state
    ├─ re-execute step "build" → still fails
    ├─ decrement retries_remaining (3 → 2)
    ├─ update status.json
    ├─ output JSON to stdout
    └─ exit (code 1)

    agent reads JSON, fixes code again...

pipelight retry --run-id <id> --step build (process 3)
    ├─ read status.json, restore pipeline state
    ├─ re-execute step "build" → success!
    ├─ continue: execute step "test" (was skipped) → success
    ├─ update status.json
    ├─ output JSON to stdout
    └─ exit (code 0)

    done!
```

### Complete LLM agent interaction example

```
Claude Code                                    pipelight
    │                                              │
    ├─ Bash: pipelight run ... --output json ──────┤
    │                                              ├─ run lint ✓
    │                                              ├─ run build ✗
    │                                              ├─ skip test (depends on build)
    │                                              ├─ write status.json
    │◄──────────── JSON output (exit 1) ───────────┤
    │                                              │
    ├─ parse JSON                                  │
    ├─ read error: "trait bound not satisfied"     │
    ├─ read context_paths: [src/, Cargo.toml]      │
    ├─ Read src/executor/mod.rs                    │
    ├─ Edit src/executor/mod.rs (fix the error)    │
    │                                              │
    ├─ Bash: pipelight retry ... --step build ─────┤
    │                                              ├─ read status.json
    │                                              ├─ re-run build ✓
    │                                              ├─ run test ✓ (unblocked)
    │                                              ├─ update status.json
    │◄──────────── JSON output (exit 0) ───────────┤
    │                                              │
    ├─ parse JSON: all steps passed                │
    └─ done                                        │
```

## CLI Commands

### Existing (modified)

```bash
# run: add --output and --run-id flags
pipelight run -f pipeline.yml --output json --run-id <id>

# --output: tty (default) | plain | json
# --run-id: optional, auto-generated UUID if omitted
```

### New commands

```bash
# retry: trigger retry of a paused step
pipelight retry --run-id <id> --step <name>

# status: check current state of a run
pipelight status --run-id <id>
pipelight status --run-id <id> --output json
```

### LLM agent workflow

```bash
# 1. Start pipeline (process runs, outputs JSON, exits)
pipelight run -f pipeline.yml --output json
# → exit code 1: retryable failure
# → JSON output includes run_id, failed step details, on_failure metadata

# 2. Agent reads JSON output, sees build failed with auto_fix strategy

# 3. Agent analyzes error_context, modifies source files

# 4. Agent triggers retry (new process, reads status.json, re-runs step, exits)
pipelight retry --run-id <run-id> --step build --output json
# → exit code 0: all passed, done
# → exit code 1: still failing, repeat 2-4
# → exit code 2: max retries exhausted, give up
```

## What Pipelight Does NOT Do

- Does NOT call any LLM API
- Does NOT modify source code
- Does NOT interpret `strategy` beyond pause/abort behavior
- Does NOT require network access (except Docker image pulls)

Pipelight is a **tool**, not an **agent**. It executes, reports, and waits. The intelligence lives outside.
