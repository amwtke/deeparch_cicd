---
name: pipelight-run
description: "Run CI/CD pipeline. Args: --reinit --skip <steps> --step <name> --dry-run --verbose. Example: /pipelight-run --skip spotbugs,pmd --verbose"
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

Arguments can be combined: `/pipelight-run --reinit --skip pmd --verbose`

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
        "callback_command": "auto_fix | auto_gen_pmd_ruleset | abort | notify",
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
2. Read `stderr` to understand the error
3. Read files listed in `on_failure.context_paths` to understand context
4. Fix the code
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

## Pipelight-misc Convention

Pipelight uses a `pipelight-misc/` directory at the project root for all CI artifacts and config files. When auto-fixing quality check failures (PMD, SpotBugs, Checkstyle), config files **must** be placed in `pipelight-misc/`, never in `src/` or project source directories:

| File | Correct Path | Wrong Path |
|------|-------------|------------|
| PMD ruleset | `pipelight-misc/pmd-ruleset.xml` | `src/pmd-ruleset.xml` |
| SpotBugs exclusions | `pipelight-misc/spotbugs-exclude.xml` | `src/spotbugs-exclude.xml` |
| Error logs | `pipelight-misc/<step>.log` | (auto-generated by pipelight) |
| PMD reports | `pipelight-misc/pmd-report/` | (auto-generated by pipelight) |
| SpotBugs reports | `pipelight-misc/spotbugs-report/` | (auto-generated by pipelight) |

The Docker container mounts the project root to `/workspace`, so `pipelight-misc/pmd-ruleset.xml` becomes `/workspace/pipelight-misc/pmd-ruleset.xml` inside the container. The pipeline steps already look for these exact paths.

## Auto-fix Boundaries

When auto-fixing failures, you may ONLY modify **application source code** (`.java`, `.py`, `.rs`, `.ts`, etc.). You must NEVER modify:

| Off-limits file | Why |
|----------------|-----|
| `pom.xml` | Build config — adding/removing plugins or dependencies changes the project's build semantics |
| `build.gradle` / `build.gradle.kts` | Same reason |
| `Cargo.toml` | Same reason |
| `package.json` | Same reason |
| `requirements.txt` / `pyproject.toml` | Same reason |
| `pipeline.yml` | Pipeline config — should only be regenerated via `pipelight init` |

**If a quality check step (PMD, SpotBugs, Checkstyle) fails because the plugin is not configured in the build file, do NOT add the plugin to `pom.xml` or `build.gradle`. Instead, report the failure and suggest the user either:**
1. Add the plugin to their build config themselves, or
2. Re-run with `--skip pmd,spotbugs` to skip those steps

**The only files auto-fix should create** are pipelight-misc config files (`pmd-ruleset.xml`, `spotbugs-exclude.xml`) to tune rule severity — never project build files.

## PMD Callback Protocol: `auto_gen_pmd_ruleset`

When the PMD step fails with `callback_command: "auto_gen_pmd_ruleset"` and stderr contains `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset`, this is a **callback** — pipelight is asking the LLM to find or generate a PMD ruleset before retrying.

### Callback Flow

```dot
digraph pmd_callback {
    "PMD step fails\nstrategy=auto_gen_pmd_ruleset" [shape=box];
    "Search project for\nexisting pmd-ruleset.xml" [shape=box];
    "Found?" [shape=diamond];
    "Copy to pipelight-misc/\npmd-ruleset.xml" [shape=box];
    "Search for coding\nguideline docs" [shape=box];
    "Found docs?" [shape=diamond];
    "Read docs & generate\npmd-ruleset.xml" [shape=box];
    "Skip PMD\n--skip pmd" [shape=box];
    "Retry PMD step" [shape=doublecircle];

    "PMD step fails\nstrategy=auto_gen_pmd_ruleset" -> "Search project for\nexisting pmd-ruleset.xml";
    "Search project for\nexisting pmd-ruleset.xml" -> "Found?";
    "Found?" -> "Copy to pipelight-misc/\npmd-ruleset.xml" [label="yes"];
    "Found?" -> "Search for coding\nguideline docs" [label="no"];
    "Copy to pipelight-misc/\npmd-ruleset.xml" -> "Retry PMD step";
    "Search for coding\nguideline docs" -> "Found docs?";
    "Found docs?" -> "Read docs & generate\npmd-ruleset.xml" [label="yes"];
    "Found docs?" -> "Skip PMD\n--skip pmd" [label="no"];
    "Read docs & generate\npmd-ruleset.xml" -> "Retry PMD step";
}
```

### Step 1: Search for Existing `pmd-ruleset.xml`

Search the project (including subdirectories/modules) for existing PMD ruleset files:

```
# Search patterns, in priority order:
pmd-ruleset.xml
pmd.xml
ruleset.xml (only in directories that suggest PMD context)
config/pmd/*.xml
**/pmd-ruleset.xml
```

For **multi-module Gradle/Maven** projects, also search inside each module directory (e.g. `module-a/config/pmd/`).

If found:
1. Copy the file to `pipelight-misc/pmd-ruleset.xml`
2. Retry the PMD step: `pipelight retry --run-id <same-id> --step pmd -f pipeline.yml --output json`

### Step 2: Search for Coding Guideline Documents

If no PMD ruleset file exists, search for coding standard/guideline documents:

```
# Search patterns:
**/coding-guide*.md
**/coding-guide*.pdf
**/coding-standard*.md
**/coding-standard*.pdf
**/code-style*.md
**/code-style*.pdf
**/CONTRIBUTING.md (may contain code style section)
**/style-guide*.md
**/checkstyle*.xml (can be partially converted)
```

If found:
1. Read the document content (for PDFs, extract the text)
2. Analyze the coding rules described in the document
3. Generate a valid PMD ruleset XML that maps the documented rules to PMD rule references
4. Write the generated ruleset to `pipelight-misc/pmd-ruleset.xml`
5. Organize supplementary files under `pipelight-misc/pmd/` if needed (e.g. `pipelight-misc/pmd/source-guideline.md` for traceability)
6. Retry the PMD step: `pipelight retry --run-id <same-id> --step pmd -f pipeline.yml --output json`

**PMD ruleset XML template:**

```xml
<?xml version="1.0"?>
<ruleset name="Project Rules"
         xmlns="http://pmd.sourceforge.net/ruleset/2.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://pmd.sourceforge.net/ruleset/2.0.0 https://pmd.sourceforge.io/ruleset_2_0_0.xsd">
    <description>Auto-generated from project coding guidelines</description>

    <!-- Map each guideline rule to a PMD rule reference -->
    <rule ref="category/java/bestpractices.xml/UnusedPrivateField" />
    <rule ref="category/java/codestyle.xml/MethodNamingConventions" />
    <!-- ... more rules based on the guideline content ... -->
</ruleset>
```

### Step 3: No Guidelines Found — Skip PMD

If neither a ruleset file nor coding guideline documents are found:
1. Report to user: "No PMD ruleset or coding guidelines found in project — skipping PMD step"
2. Re-run the pipeline with `--skip pmd`: `pipelight run -f pipeline.yml --output json --run-id <new-id> --skip pmd`

### Handling Subsequent PMD Failures (Violations)

After the LLM successfully places a ruleset and retries, the PMD step may fail again — this time with **actual PMD violations** (stderr will NOT contain `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset`). In this case, treat it like a normal `auto_fix` failure:
1. Read the violations from stderr/stdout
2. Fix the source code
3. Retry the PMD step

## Common Mistakes

| Mistake | Correct Approach |
|---------|-----------------|
| Omit `--output json` | Always use `--output json` for machine parsing |
| Omit `--run-id` | Always set `--run-id` so retry can reference it |
| Retry without `--step` | `--step` is required for retry command |
| Retry when `retries_remaining == 0` | Check before retrying, report failure instead |
| Fix code without reading `context_paths` | Always read context files first for full understanding |
| Retry `failed` (non-retryable) pipeline | Only retry when status is `retryable` |
| Modify `pom.xml` / `build.gradle` during auto-fix | Never touch build config files — only fix source code or add pipelight-misc config |
