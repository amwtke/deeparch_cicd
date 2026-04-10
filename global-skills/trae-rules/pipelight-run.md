---
description: "Run CI/CD pipeline with pipelight. Trigger when user says 'run pipeline', 'build', 'CI check', 'pipelight', or wants to verify code changes. Args: --reinit --skip <steps> --step <name> --dry-run --verbose."
globs: "pipeline.yml,*.java,*.py,*.rs,*.ts,*.go"
alwaysApply: false
---

# Pipelight-Run: CI/CD Pipeline Execution Protocol

## Overview

Pipelight is the project's lightweight CLI CI/CD tool. This rule defines the interaction protocol: run pipeline with JSON output, parse results, auto-fix on retryable failures, and retry until success or exhaustion.

## When to Apply

- User says "run pipeline" / "build" / "CI check" / "pipelight"
- User wants to verify code changes compile/pass tests
- After making code changes that need validation
- When a previous pipelight run returned `retryable` and you need to fix + retry

## Core Flow

```
Run pipelight --output json
    → Parse JSON
    → status?
        success → Done - report success
        failed  → Done - report failure
        retryable → Read stderr + context_paths
                  → Fix code
                  → retries_remaining > 0?
                      yes → pipelight retry --output json → Parse JSON (loop)
                      no  → Done - report failure
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `--reinit` | Force regenerate `pipeline.yml` before running | `--reinit` |
| `--skip <steps>` | Skip one or more steps (comma-separated) | `--skip spotbugs,pmd` |
| `--step <name>` | Run only a specific step | `--step build` |
| `--dry-run` | Show execution plan without running | `--dry-run` |
| `--verbose` | Show full container output | `--verbose` |

Arguments can be combined: `--reinit --skip pmd --verbose`

## Step 1: Check pipeline.yml Exists

If the project has no `pipeline.yml`, **or `--reinit` is requested**, generate one:

```bash
pipelight init -d .
```

When `--reinit` is used, this overwrites the existing `pipeline.yml` with a freshly detected configuration.

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
      "error_context": { "files": ["..."], "lines": ["..."], "error_type": "..." },
      "on_failure": {
        "exception_key": "compile_error",
        "command": "auto_fix",
        "action": "retry",
        "max_retries": 3,
        "retries_remaining": 3,
        "context_paths": ["src/", "Cargo.toml"]
      },
      "test_summary": { "passed": 42, "failed": 3, "skipped": 1 }
    }
  ]
}
```

## Step 4: Act on Status

### `status: "success"`

Report success to user. Show step durations if relevant.

### `status: "failed"`

Pipeline failed with no auto-fix strategy. Report the error:
- Show which step failed
- Show `stderr` content
- Show `error_context` if present
- Do NOT attempt auto-fix (strategy is `abort` or `notify`)

### `status: "retryable"`

Pipeline failed but auto-fix is configured. Enter fix-retry loop:

1. Find the failed step (the one with `status: "failed"`)
2. Read `on_failure.command` — look it up in the **Command Mapping Table** above
3. Execute the command's **LLM reasoning instructions**
4. Read files listed in `on_failure.context_paths` to understand context
5. Check `retries_remaining > 0` before retrying
6. Run retry:

```bash
pipelight retry --run-id <same-run-id> --step <failed-step-name> -f pipeline.yml --output json
```

7. Parse the new JSON result and repeat from Step 4

## Exit Code Reference

| Exit Code | Meaning |
|-----------|---------|
| 0 | Pipeline succeeded |
| 1 | Pipeline retryable (has auto_fix steps with retries left) |
| 2 | Pipeline failed (abort/notify, or retries exhausted) |

## Callback Command Reference

When a step fails, the JSON `on_failure` object contains three layers of information:

| Field | Meaning |
|-------|---------|
| `exception_key` | What went wrong (step-internal identifier) |
| `command` | The CallbackCommand that was resolved (determines LLM reasoning strategy) |
| `action` | The behavior type: `retry`, `abort`, or `notify` |

### Action Quick Reference

| action | LLM behavior |
|--------|-------------|
| `retry` | Execute the command's reasoning instructions below, then `pipelight retry --step <name>` |
| `abort` | Report error to user, do not attempt fix |
| `notify` | Report results to user, do not attempt fix |

### Command Mapping Table

| command | action | LLM reasoning instructions |
|---------|--------|---------------------------|
| `auto_fix` | retry | 1. Read stderr to understand error 2. Read files in context_paths 3. Fix source code only (never build config) 4. Retry |
| `auto_gen_pmd_ruleset` | retry | 1. Search project for existing PMD ruleset 2. Found → copy to `pipelight-misc/pmd-ruleset.xml` → retry 3. Not found → search for coding guideline docs 4. Found docs → read, generate PMD 7.x ruleset XML → retry 5. Nothing found → report and `--skip pmd` |
| `abort` | abort | Report failed step + stderr + error_context to user |
| `notify` | notify | Report test results / summary to user |

## Pipelight-misc Convention

Pipelight uses a `pipelight-misc/` directory for all CI artifacts and config files. **This directory MUST be located at the root of the target project** — the same directory where `pipeline.yml` resides and where `pipelight run` is executed.

**CRITICAL:** When creating config files (ruleset, exclusion filters), always use the **absolute path** to the target project's `pipelight-misc/` directory.

| File | Correct Path | Wrong Path |
|------|-------------|------------|
| PMD ruleset | `<project-root>/pipelight-misc/pmd-ruleset.xml` | `src/pmd-ruleset.xml` |
| SpotBugs exclusions | `<project-root>/pipelight-misc/spotbugs-exclude.xml` | `src/spotbugs-exclude.xml` |
| Error logs | `<project-root>/pipelight-misc/<step>.log` | (auto-generated) |
| PMD reports | `<project-root>/pipelight-misc/pmd-report/` | (auto-generated) |
| SpotBugs reports | `<project-root>/pipelight-misc/spotbugs-report/` | (auto-generated) |

The Docker container mounts the target project root to `/workspace`, so `<project-root>/pipelight-misc/pmd-ruleset.xml` becomes `/workspace/pipelight-misc/pmd-ruleset.xml` inside the container.

**How to find the target project root:**

```bash
PROJECT_ROOT="$(dirname "$(realpath pipeline.yml)")"
```

## Auto-fix Boundaries

When auto-fixing failures, you may ONLY modify **application source code** (`.java`, `.py`, `.rs`, `.ts`, etc.). You must NEVER modify:

| Off-limits file | Why |
|----------------|-----|
| `pom.xml` | Build config — changes build semantics |
| `build.gradle` / `build.gradle.kts` | Same reason |
| `Cargo.toml` | Same reason |
| `package.json` | Same reason |
| `requirements.txt` / `pyproject.toml` | Same reason |
| `pipeline.yml` | Pipeline config — only regenerate via `pipelight init` |

**If a quality check step (PMD, SpotBugs, Checkstyle) fails because the plugin is not configured in the build file, do NOT add the plugin. Instead, report the failure and suggest the user either:**
1. Add the plugin to their build config themselves, or
2. Re-run with `--skip pmd,spotbugs` to skip those steps

**The only files auto-fix should create** are pipelight-misc config files (`pmd-ruleset.xml`, `spotbugs-exclude.xml`).

## PMD Callback Protocol: `auto_gen_pmd_ruleset`

When the PMD step fails with `callback_command: "auto_gen_pmd_ruleset"` and stderr contains `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset`, pipelight is asking you to find or generate a PMD ruleset before retrying.

### Callback Flow

```
PMD step fails (strategy=auto_gen_pmd_ruleset)
    → Search project for existing pmd-ruleset.xml
    → Found?
        yes → Copy to pipelight-misc/pmd-ruleset.xml → Retry
        no  → Search for coding guideline docs
            → Found docs?
                yes → Read docs, generate pmd-ruleset.xml → Retry
                no  → Skip PMD (--skip pmd)
```

### Search for Existing PMD Ruleset

Search patterns, in priority order:
- `pmd-ruleset.xml`
- `pmd.xml`
- `ruleset.xml` (in PMD-related directories)
- `config/pmd/*.xml`
- `**/pmd-ruleset.xml`

For multi-module projects, also search inside each module directory.

If found: copy to `<project-root>/pipelight-misc/pmd-ruleset.xml` and retry.

### Search for Coding Guideline Documents

If no PMD ruleset exists, search for coding standard documents:

```
**/coding-guide*.md, **/coding-guide*.pdf
**/coding-standard*.md, **/coding-standard*.pdf
**/code-style*.md, **/code-style*.pdf
**/CONTRIBUTING.md
**/style-guide*.md
**/checkstyle*.xml

# Chinese naming patterns:
**/*编码规范*.md, **/*编码规范*.pdf
**/*代码规范*.md, **/*代码规范*.pdf
**/*编码标准*.md, **/*开发规范*.md
**/*代码风格*.md, **/*编程规范*.md
```

Also search broadly in `docs/`, `doc/`, `standards/`, `guidelines/` directories.

If found: read content, generate PMD ruleset XML mapping rules to PMD references, write to `<project-root>/pipelight-misc/pmd-ruleset.xml`, and retry.

**PMD ruleset XML template:**

```xml
<?xml version="1.0"?>
<ruleset name="Project Rules"
         xmlns="http://pmd.sourceforge.net/ruleset/2.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://pmd.sourceforge.net/ruleset/2.0.0 https://pmd.sourceforge.io/ruleset_2_0_0.xsd">
    <description>Auto-generated from project coding guidelines</description>
    <rule ref="category/java/bestpractices.xml/UnusedPrivateField" />
    <rule ref="category/java/codestyle.xml/MethodNamingConventions" />
</ruleset>
```

### No Guidelines Found — Skip PMD

If neither ruleset nor guidelines found:
1. Report: "No PMD ruleset or coding guidelines found — skipping PMD step"
2. Re-run with `--skip pmd`

### Handling Subsequent PMD Violations

After ruleset is placed and retry succeeds in finding PMD, but violations are reported (stderr will NOT contain `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset`), treat as normal `auto_fix`:
1. Read violations from stderr/stdout
2. Fix the source code
3. Retry the PMD step

## Common Mistakes

| Mistake | Correct Approach |
|---------|-----------------|
| Omit `--output json` | Always use `--output json` for machine parsing |
| Omit `--run-id` | Always set `--run-id` so retry can reference it |
| Retry without `--step` | `--step` is required for retry command |
| Retry when `retries_remaining == 0` | Check before retrying, report failure instead |
| Fix code without reading `context_paths` | Always read context files first |
| Retry `failed` (non-retryable) pipeline | Only retry when status is `retryable` |
| Modify `pom.xml` / `build.gradle` during auto-fix | Never touch build config — only fix source code or add pipelight-misc config |
