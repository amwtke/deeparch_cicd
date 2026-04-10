---
name: pipelight-run
description: "Run CI/CD pipeline. Args: --reinit --skip <steps> --step <name> --dry-run --verbose --docker-prepare --clean. Example: /pipelight-run --skip spotbugs,pmd --verbose"
---

# /pipelight-run

## Overview

Pipelight is the project's lightweight CLI CI/CD tool. This skill defines the interaction protocol: run pipeline with JSON output, parse results, auto-fix on retryable failures, and retry until success or exhaustion.

## When to Use

- User says "run pipeline" / "build" / "CI check" / "pipelight"
- User wants to verify code changes compile/pass tests
- After making code changes that need validation
- When a previous pipelight run returned `retryable` and you need to fix + retry

## Core Flow

```dot
digraph pipelight {
    "Run pipelight\n--output json" [shape=box];
    "Parse JSON" [shape=box];
    "status?" [shape=diamond];
    "Done - report success" [shape=doublecircle];
    "Done - report failure" [shape=doublecircle];
    "Read stderr +\ncontext_paths" [shape=box];
    "Fix code" [shape=box];
    "retries_remaining > 0?" [shape=diamond];
    "pipelight retry\n--output json" [shape=box];

    "Run pipelight\n--output json" -> "Parse JSON";
    "Parse JSON" -> "status?";
    "status?" -> "Done - report success" [label="success"];
    "status?" -> "Done - report failure" [label="failed"];
    "status?" -> "Read stderr +\ncontext_paths" [label="retryable"];
    "Read stderr +\ncontext_paths" -> "Fix code";
    "Fix code" -> "retries_remaining > 0?";
    "retries_remaining > 0?" -> "pipelight retry\n--output json" [label="yes"];
    "retries_remaining > 0?" -> "Done - report failure" [label="no"];
    "pipelight retry\n--output json" -> "Parse JSON";
}
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `--reinit` | Force regenerate `pipeline.yml` before running | `/pipelight-run --reinit` |
| `--skip <steps>` | Skip one or more steps (comma-separated) | `/pipelight-run --skip spotbugs,pmd` |
| `--step <name>` | Run only a specific step | `/pipelight-run --step build` |
| `--dry-run` | Show execution plan without running | `/pipelight-run --dry-run` |
| `--verbose` | Show full container output | `/pipelight-run --verbose` |
| `--docker-prepare` | Pull all Docker images from pipeline.yml without running pipeline | `/pipelight-run --docker-prepare` |
| `--clean` | Remove pipeline.yml and pipelight-misc/ from current project | `/pipelight-run --clean` |

Arguments can be combined: `/pipelight-run --reinit --skip pmd --verbose`

## Clean Mode

When `--clean` is passed, **do NOT run the pipeline**. Instead run:

```bash
pipelight clean
```

This removes `pipeline.yml` and `pipelight-misc/` from the current project directory. Does not affect global cache (`~/.pipelight/cache/`).

## Docker Prepare Mode

When `--docker-prepare` is passed, **do NOT run the pipeline**. Instead:

1. Check `pipeline.yml` exists (generate if needed, same as Step 1)
2. Parse `pipeline.yml` and collect all unique `image` values from steps (skip steps with `local: true` or empty image)
3. For each image, run `docker pull <image>` and report progress
4. Report summary of pulled images

This is useful when the user needs to pre-pull images on a network that can reach Docker Hub (e.g. before connecting to a VPN that blocks it).

```bash
# Example flow:
docker pull rust:latest
docker pull alpine/git:latest
# ... etc
```

After `--docker-prepare` completes, the user can switch networks and run `/pipelight-run` normally — Docker will use cached images.

## Step 1: Check pipeline.yml Exists

If the project has no `pipeline.yml`, **or the user passed `--reinit`**, generate one:

```bash
pipelight init -d .
```

When `--reinit` is used, this overwrites the existing `pipeline.yml` with a freshly detected configuration.

Review the generated file and adjust if needed.

## Step 2: Run Pipeline

```bash
pipelight run -f pipeline.yml --output json --run-id <short-id>
```

- Always use `--output json` so output is machine-parseable
- Always provide `--run-id` (e.g. `run-001`) to enable retry
- Use `-f` to point to the correct pipeline file if not `pipeline.yml`
- If `--skip` was passed, add `--skip <step1> <step2>` to skip those steps
- If `--step` was passed, add `--step <name>` to run only that step
- If `--dry-run` was passed, add `--dry-run` to show plan without executing
- If `--verbose` was passed, add `--verbose` to show full container output

## Step 3: Parse JSON Result

**IMPORTANT: 每次收到 pipelight 的 JSON 输出（包括首次运行和每次 retry），都必须将完整 JSON 原文打印给用户。** 使用 JSON code block 展示，让用户能看到 LLM 与 pipelight 之间的每一次完整交互。如果 JSON 输出过大被截断，则从工具结果文件中提取关键字段（run_id, status, 每个 step 的 name/status/report_summary/stderr）并以 JSON 格式打印。

JSON structure:

```json
{
  "run_id": "run-001",
  "pipeline": "rust-ci",
  "status": "success | failed | retryable",
  "duration_ms": 5000,
  "steps": [
    {
      "name": "build",
      "status": "success | failed | skipped | pending | running",
      "exit_code": 0,
      "duration_ms": 3000,
      "image": "rust:1.78-slim",
      "command": "cargo build --release",
      "stdout": "...",
      "stderr": "...",
      "error_context": { "files": [...], "lines": [...], "error_type": "..." },
      "on_failure": {
        "strategy": "autofix | abort | notify",
        "max_retries": 3,
        "retries_remaining": 3,
        "context_paths": ["src/", "Cargo.toml"]
      },
      "test_summary": { "passed": 42, "failed": 3, "skipped": 1 },
      "report_summary": "Compiled successfully",
      "report_path": "pipelight-misc/build-20260410T002026.log"
    }
  ]
}
```

## Step 4: Act on Status

### `status: "success"`

Report success to user with a summary table including Step, Status, and Summary columns. The Summary column shows each step's `report_summary` field from the JSON output.

Example:

| Step | Status | Summary |
|------|--------|---------|
| git-pull | skipped | — |
| build | success | Compiled successfully |
| pmd | success | PMD Total: 0 violations |
| spotbugs | success | SpotBugs Total: 0 bugs found |
| test | success | Tests: 42 passed, 0 failed |
| package | success | Packaged successfully |

### `status: "failed"`

Pipeline failed with no auto-fix strategy. Report the error:
- Show which step failed
- Show `stderr` content
- Show `error_context` if present
- Do NOT attempt auto-fix (strategy is `abort` or `notify`)

### `status: "retryable"`

Pipeline failed but auto-fix is configured. Enter fix-retry loop. **Each round of the loop MUST be printed to the user** so they can see the full discovery → fix → retry process.

#### Fix-Retry Loop

1. Find the failed step (the one with `status: "failed"`)
2. **Print diagnosis** — show what failed and why:

```
### <step-name> failed (retryable, N retries remaining)

**Error:** <one-line error summary from stderr>
**File:** <file:line if available from error_context or stderr>
```

3. Read `stderr` to understand the error
4. Read files listed in `on_failure.context_paths` to understand context
5. **Print what you're fixing** — show the cause and the fix:

```
**Cause:** <root cause explanation>
**Fix:** <what you changed>
**Files modified:**
- `path/to/file.java` — <brief description of change>
```

6. Fix the code
7. Check `retries_remaining > 0` before retrying
8. **Print retry action:**

```
### Retrying <step-name>...
```

9. Run retry:

```bash
pipelight retry --run-id <same-run-id> --step <failed-step-name> -f pipeline.yml --output json
```

10. Parse the new JSON result and repeat from Step 4

#### Multiple Rounds

If multiple retry rounds occur, print each round sequentially so the user sees the full history. Number them: `### Round 1`, `### Round 2`, etc.

### Success Report (after retries)

When the pipeline eventually succeeds after one or more fix-retry rounds, the final summary table MUST include an **Auto-fix History** section below the step table, listing all files that were modified during the fix-retry loop:

Example:

| Step | Status | Summary |
|------|--------|---------|
| build | success | Compiled successfully |
| pmd | success | PMD Total: 0 violations |
| test | success | Tests: 42 passed, 0 failed |

**Auto-fix History (1 round):**
- `src/com/example/Foo.java:128` — removed stray junk text `ddddddd` causing syntax error
- `src/com/example/Bar.java:45` — fixed missing semicolon

If no auto-fix occurred (pipeline passed on first run), omit this section entirely.

## Guardrails

### Never Execute Pipeline Commands Directly

When a step fails, you must ONLY:
1. Read stderr and context_paths to understand the error
2. Fix the source code (edit files)
3. Retry via `pipelight retry`

**NEVER** execute pipeline step commands directly on the host (e.g., `cargo fmt`, `cargo build`, `mvn compile`, `npm run build`). All step commands must run through the pipelight pipeline inside Docker containers.

**Why:** Direct execution bypasses Docker isolation, skips the pipeline's reporting/retry mechanism, and produces results that differ from the pipeline environment. It also creates local file modifications that the user didn't ask for.

**What to do instead:**
- If `status: "retryable"` → enter fix-retry loop (edit code, then `pipelight retry`)
- If `status: "failed"` (non-retryable) → report the error, do NOT attempt to fix

## Exit Code Reference

| Exit Code | Meaning |
|-----------|---------|
| 0 | Pipeline succeeded |
| 1 | Pipeline retryable (has auto_fix steps with retries left) |
| 2 | Pipeline failed (abort/notify, or retries exhausted) |

## Common Mistakes

| Mistake | Correct Approach |
|---------|-----------------|
| Omit `--output json` | Always use `--output json` for machine parsing |
| Omit `--run-id` | Always set `--run-id` so retry can reference it |
| Retry without `--step` | `--step` is required for retry command |
| Retry when `retries_remaining == 0` | Check before retrying, report failure instead |
| Fix code without reading `context_paths` | Always read context files first for full understanding |
| Retry `failed` (non-retryable) pipeline | Only retry when status is `retryable` |
| Execute step commands directly (e.g., `cargo fmt`) | Only fix source code and retry via `pipelight retry`. Never run step commands outside the pipeline |
