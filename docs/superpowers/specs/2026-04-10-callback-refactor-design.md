# Callback Command Two-Layer Mapping Refactor

**Date:** 2026-04-10
**Status:** Approved
**Scope:** `src/ci/callback/` (new module) + StepDef trait changes + CLI dispatcher changes + pipelight-run skill update

## Problem

Current callback architecture has three issues:

1. **Action semantics hardcoded in CLI**: `cli/mod.rs:475` matches `CallbackCommand` variants to determine retry vs abort — this logic belongs to the command itself, not the dispatcher.
2. **Static one-to-one binding**: Each step can only declare one `callback_command` at build time via `config()`. In reality, a step can fail for different reasons requiring different responses (e.g., PMD: download failed → abort; ruleset missing → auto_gen).
3. **No separation of concerns**: `CallbackCommand` conflates what the command is with what action to take. The action (retry/abort/notify) should be a property of the command definition, not external match logic.

## Domain Terms

| Term | Rust Type | Role |
|------|-----------|------|
| **CallbackCommandAction** | `enum { Retry, Abort, Notify }` | LLM interaction protocol — behavior part. Compile-time fixed; the scheduler's three fundamental responses. |
| **CallbackCommand** | `enum { Abort, Notify, AutoFix, AutoGenPmdRuleset }` | LLM interaction protocol — command part. Compile-time enum; LLM uses this to decide which reasoning strategy to apply. New commands added by extending the enum. |
| **CallbackCommandDef** | `struct` | Definition of a CallbackCommand: its action + description for LLM. |
| **CallbackCommandRegistry** | `struct` wrapping `HashMap<CallbackCommand, CallbackCommandDef>` | Global registry mapping each command variant to its definition. Built-in commands registered at startup. |
| **ExceptionMapping** | `struct` per step | Maps exception keys to `CallbackCommand` + retry policy. Each step defines its own. |
| **ExceptionEntry** | `struct` | One mapping entry: `exception_key → { command: CallbackCommand, max_retries, context_paths }`. |
| **ResolvedFailure** | `struct` | Result of exception resolution: the matched exception key, command (`CallbackCommand`), and retry policy. |

## Architecture

### Three-Layer Model

```
Step Exception (per-step)  →  CallbackCommand (global registry)  →  CallbackCommandAction
   exception_key               command string                        Retry / Abort / Notify
```

Data flows left to right. Each layer has a single responsibility:

- **Exception layer**: Step knows what can go wrong and which command to invoke for each case
- **Command layer**: Global registry maps command names to actions + LLM descriptions
- **Action layer**: Scheduler knows three behaviors — retry, abort, notify

### Module Structure

```
src/ci/
  callback/                  ← NEW: standalone module, zero business coupling
    mod.rs                   ← Public exports
    action.rs                ← CallbackCommandAction enum
    command.rs               ← CallbackCommandDef + CallbackCommandRegistry
    exception.rs             ← ExceptionMapping + ExceptionEntry + ResolvedFailure + resolve()
  detector/                  ← Unchanged
  executor/                  ← Unchanged
  output/                    ← Unchanged
  parser/                    ← CallbackCommand enum preserved for YAML compat, converted internally
  pipeline_builder/          ← StepDef trait gains exception_mapping() + match_exception()
  scheduler/                 ← Unchanged
```

### Dependency Graph

```
callback/  ← Zero external dependencies, leaf module
    ↑
pipeline_builder/  → depends on callback (StepDef returns ExceptionMapping)
    ↑
cli/  → depends on callback (CallbackCommandRegistry to resolve actions)
      → depends on pipeline_builder (to get step_def for exception resolution)
```

`callback/` does NOT import any other business module.

## Detailed Design

### 1. CallbackCommandAction (`action.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommandAction {
    Retry,   // LLM executes fix, then pipelight retries the step
    Abort,   // Pipeline terminates, unrecoverable
    Notify,  // Pipeline terminates, notify user of results
}
```

Compile-time fixed. Plugins cannot and should not extend this — these are the scheduler's three fundamental behavioral responses.

### 2. CallbackCommand + CallbackCommandDef + CallbackCommandRegistry (`command.rs`)

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use super::action::CallbackCommandAction;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommand {
    Abort,
    Notify,
    AutoFix,
    AutoGenPmdRuleset,
    // Future commands added here as new variants
}

pub struct CallbackCommandDef {
    pub action: CallbackCommandAction,
    pub description: String,  // Semantic description for LLM
}

pub struct CallbackCommandRegistry {
    commands: HashMap<CallbackCommand, CallbackCommandDef>,
}

impl CallbackCommandRegistry {
    /// Create registry with built-in commands
    pub fn new() -> Self {
        let mut registry = Self { commands: HashMap::new() };
        registry.register(CallbackCommand::Abort, CallbackCommandDef {
            action: CallbackCommandAction::Abort,
            description: "Unrecoverable error. Pipeline terminates.".into(),
        });
        registry.register(CallbackCommand::Notify, CallbackCommandDef {
            action: CallbackCommandAction::Notify,
            description: "Notify user of results (e.g., test summary). Pipeline terminates.".into(),
        });
        registry.register(CallbackCommand::AutoFix, CallbackCommandDef {
            action: CallbackCommandAction::Retry,
            description: "LLM reads stderr + context_paths, fixes source code, then retries.".into(),
        });
        registry.register(CallbackCommand::AutoGenPmdRuleset, CallbackCommandDef {
            action: CallbackCommandAction::Retry,
            description: "LLM searches project for existing PMD ruleset or coding guidelines, generates pmd-ruleset.xml, then retries.".into(),
        });
        registry
    }

    /// Register a command definition
    pub fn register(&mut self, command: CallbackCommand, def: CallbackCommandDef) {
        self.commands.insert(command, def);
    }

    /// Look up a command definition
    pub fn get(&self, command: &CallbackCommand) -> Option<&CallbackCommandDef> {
        self.commands.get(command)
    }

    /// Get the action for a command.
    pub fn action_for(&self, command: &CallbackCommand) -> CallbackCommandAction {
        self.commands
            .get(command)
            .map(|def| def.action.clone())
            .unwrap_or(CallbackCommandAction::Abort)
    }
}
```

### 3. ExceptionMapping + Resolution (`exception.rs`)

```rust
use std::collections::HashMap;
use super::command::CallbackCommand;

pub struct ExceptionEntry {
    pub command: CallbackCommand,     // Enum variant, not String
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}

pub struct ExceptionMapping {
    entries: HashMap<String, ExceptionEntry>,       // exception_key (String) → entry
    default_command: CallbackCommand,               // Fallback when no exception matches
}

pub struct ResolvedFailure {
    pub exception_key: String,
    pub command: CallbackCommand,
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}

impl ExceptionMapping {
    pub fn new(default_command: CallbackCommand) -> Self {
        Self {
            entries: HashMap::new(),
            default_command,
        }
    }

    pub fn add(mut self, key: &str, entry: ExceptionEntry) -> Self {
        self.entries.insert(key.into(), entry);
        self
    }

    /// Resolution chain:
    /// 1. Parse stderr for PIPELIGHT_EXCEPTION:<key> marker
    /// 2. Call match_fn (StepDef::match_exception) for Rust-side analysis
    /// 3. Fallback to default_command → unrecognized exception (fatal)
    pub fn resolve(
        &self,
        exit_code: i64,
        stdout: &str,
        stderr: &str,
        match_fn: Option<&dyn Fn(i64, &str, &str) -> Option<String>>,
    ) -> ResolvedFailure {
        // Priority 1: stderr marker
        let exception_key = Self::parse_stderr_marker(stderr)
            // Priority 2: Rust match function
            .or_else(|| match_fn.and_then(|f| f(exit_code, stdout, stderr)));

        match exception_key {
            Some(key) if self.entries.contains_key(&key) => {
                let entry = &self.entries[&key];
                ResolvedFailure {
                    exception_key: key,
                    command: entry.command.clone(),
                    max_retries: entry.max_retries,
                    context_paths: entry.context_paths.clone(),
                }
            }
            _ => {
                // Fallback: unrecognized or unmapped exception
                let key = exception_key.unwrap_or_else(|| "unrecognized".into());
                ResolvedFailure {
                    exception_key: key,
                    command: self.default_command.clone(),
                    max_retries: 0,
                    context_paths: vec![],
                }
            }
        }
    }

    fn parse_stderr_marker(stderr: &str) -> Option<String> {
        // Look for PIPELIGHT_EXCEPTION:<key> pattern in stderr
        for line in stderr.lines() {
            if let Some(rest) = line.strip_prefix("PIPELIGHT_EXCEPTION:") {
                let key = rest.split_whitespace().next().unwrap_or(rest).trim();
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
        None
    }
}
```

Note: exception keys remain `String` because they are step-internal identifiers parsed from stderr markers — they are not part of the LLM protocol and don't benefit from enum enforcement.

### 4. StepDef Trait Changes (`pipeline_builder/mod.rs`)

```rust
use crate::ci::callback::exception::ExceptionMapping;

pub trait StepDef: Send + Sync {
    fn config(&self) -> StepConfig;
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String;

    /// Return the exception-to-command mapping for this step.
    /// Default: empty mapping with Abort fallback (all failures are fatal).
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }

    /// Analyze execution output to identify the exception key.
    /// Called as priority 2 in the resolution chain (after stderr marker).
    /// Default: None (no Rust-side analysis).
    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        None
    }
}
```

### 5. StepConfig Changes

Remove `on_failure: Option<OnFailure>` from `StepConfig`. Exception handling is no longer part of step configuration — it is provided independently via `exception_mapping()`.

```rust
pub struct StepConfig {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
    pub allow_failure: bool,
    pub volumes: Vec<String>,
    pub local: bool,
    // on_failure: REMOVED
}
```

### 6. CLI Dispatcher Changes (`cli/mod.rs`)

Replace the hardcoded match:

```rust
// BEFORE (delete this)
match callback_cmd {
    CallbackCommand::AutoFix | CallbackCommand::AutoGenPmdRuleset => {
        has_retryable_failure = true;
    }
    CallbackCommand::Abort | CallbackCommand::Notify => {
        has_final_failure = true;
    }
}
```

With registry-based resolution:

```rust
// AFTER
let resolved = step_def.exception_mapping().resolve(
    result.exit_code,
    &stdout,
    &stderr,
    Some(&|ec, out, err| step_def.match_exception(ec, out, err)),
);
let action = registry.action_for(&resolved.command);
match action {
    CallbackCommandAction::Retry => { has_retryable_failure = true; }
    CallbackCommandAction::Abort => { has_final_failure = true; }
    CallbackCommandAction::Notify => { has_final_failure = true; }
}
```

The CLI only knows `CallbackCommandAction` — fully decoupled from specific commands.

### 7. OnFailureState Changes (`run_state/mod.rs`)

JSON output becomes richer — LLM sees all three layers:

```rust
use crate::ci::callback::action::CallbackCommandAction;
use crate::ci::callback::command::CallbackCommand;

pub struct OnFailureState {
    pub exception_key: String,                // What went wrong (step-internal key)
    pub command: CallbackCommand,             // Enum: which CallbackCommand was resolved
    pub action: CallbackCommandAction,        // Enum: Retry / Abort / Notify
    pub max_retries: u32,
    pub retries_remaining: u32,
    pub context_paths: Vec<String>,
}
```

Both `command` and `action` are enums with `#[serde(rename_all = "snake_case")]`, so JSON serialization produces clean strings automatically:

Example JSON output:

```json
{
  "on_failure": {
    "exception_key": "ruleset_not_found",
    "command": "auto_gen_pmd_ruleset",
    "action": "retry",
    "max_retries": 2,
    "retries_remaining": 2,
    "context_paths": ["src/main/java"]
  }
}
```

### 8. YAML Parser Compatibility (`parser/mod.rs`)

The `CallbackCommand` enum and `OnFailure` struct in `parser/` are preserved for user-written `pipeline.yml` files. When converting to internal representation, map to the new system:

The `CallbackCommand` enum in `parser/mod.rs` is **replaced** by the new `callback::command::CallbackCommand` enum (same variants, same serde behavior). The parser directly deserializes into the new enum. No conversion layer needed — one canonical type throughout the codebase.

The old `parser::CallbackCommand` enum is deleted. `parser::OnFailure` now references `callback::command::CallbackCommand`:

```rust
// parser/mod.rs
use crate::ci::callback::command::CallbackCommand;

pub struct OnFailure {
    pub callback_command: CallbackCommand,  // Uses the canonical enum
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}
```

This preserves backward compatibility for hand-written YAML pipelines while eliminating type duplication.

### 9. PMD Step Example (Refactored)

```rust
// maven/pmd_step.rs
impl StepDef for PmdStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![/* unchanged shell script */],
            depends_on: vec!["build".into()],
            ..Default::default()
        }
        // NOTE: no on_failure here anymore
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)  // fallback: fatal
            .add("ruleset_not_found", ExceptionEntry {
                command: CallbackCommand::AutoGenPmdRuleset,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
            .add("ruleset_invalid", ExceptionEntry {
                command: CallbackCommand::AutoGenPmdRuleset,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("Cannot load ruleset") || stderr.contains("Unable to find referenced rule") {
            Some("ruleset_invalid".into())
        } else if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
            // Backward compat with old stderr marker format
            Some("ruleset_not_found".into())
        } else {
            None  // Falls through to default → abort
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        // unchanged
    }
}
```

Shell script migration: `PIPELIGHT_CALLBACK:xxx` markers should be gradually migrated to `PIPELIGHT_EXCEPTION:xxx` format. Both are supported during transition via `match_exception()`.

## Pipelight-Run Skill Changes

### New Section: Callback Command Reference

Replace the scattered callback handling logic in the skill with a unified **Callback Command Reference** section. The LLM's entry point for handling any failure is always this mapping table.

### Behavior Quick Reference

| action | LLM behavior |
|--------|-------------|
| `retry` | Execute the command's reasoning instructions, then `pipelight retry --step <name>` |
| `abort` | Report error to user, do not attempt fix |
| `notify` | Report results to user, do not attempt fix |

### Command Mapping Table

| command | action | LLM reasoning instructions |
|---------|--------|---------------------------|
| `auto_fix` | retry | 1. Read stderr to understand error 2. Read files in context_paths 3. Fix source code only (never build config) 4. Retry |
| `auto_gen_pmd_ruleset` | retry | 1. Search project for existing PMD ruleset (`**/pmd-ruleset.xml`, `**/pmd.xml`, `config/pmd/*.xml`) 2. Found → copy to `pipelight-misc/pmd-ruleset.xml` → retry 3. Not found → search for coding guideline docs (`docs/`, `doc/`, `*guideline*`, `*coding*`) 4. Found docs → read content, generate PMD 7.x ruleset XML → retry 5. Nothing found → report and `--skip pmd` |
| `abort` | abort | Report failed step + stderr + error_context to user |
| `notify` | notify | Report test results / summary to user |

Each command's detailed sub-flow is documented in its own subsection below the table (e.g., PMD search rules, auto-fix boundaries). The table is the LLM's lookup entry point; subsections provide depth.

### Core Flow Update

```
Step 4: Act on Status
    success   → Report success
    failed    → on_failure.action == "abort" or "notify" → report to user
    retryable → Look up on_failure.command in Command Mapping Table
              → Execute the command's "LLM reasoning instructions"
              → Check retries_remaining > 0
              → pipelight retry --step <name> --run-id <id> --output json
              → Parse JSON, loop back to Step 4
```

### Extensibility

When a plugin registers a new CallbackCommand (e.g., `auto_gen_dockerfile`):
1. **Rust side**: Plugin calls `registry.register("auto_gen_dockerfile", CallbackCommandDef { action: Retry, description: "..." })`
2. **Skill side**: Add one row to the Command Mapping Table with the LLM reasoning instructions

No changes to CLI dispatcher code. No changes to the skill's core flow.

## Migration Strategy

### Phase 1: Add `callback/` module
- Create `src/ci/callback/` with `action.rs`, `command.rs`, `exception.rs`
- All new code, no existing code breaks

### Phase 2: Evolve StepDef trait
- Add `exception_mapping()` and `match_exception()` with defaults
- Existing steps continue to work (defaults return empty mapping → abort fallback)

### Phase 3: Migrate steps one by one
- For each step: implement `exception_mapping()`, remove `on_failure` from `config()`
- Start with PMD step (most complex, best validates the design)
- Then build/lint/fmt (auto_fix), then test (notify), then git_pull/package (abort)

### Phase 4: Update CLI dispatcher
- Replace hardcoded match with registry-based resolution
- Update `OnFailureState` with new fields (`exception_key`, `command`, `action`)
- JSON backward compatibility: keep existing `callback_command` field as a deprecated alias of `command` for one release cycle, then remove

### Phase 5: Update pipelight-run skill
- Add Callback Command Reference section
- Restructure existing PMD/auto-fix docs as command subsections
- Remove scattered callback handling from core flow

### Phase 6: Migrate stderr markers
- Gradually change shell scripts from `PIPELIGHT_CALLBACK:xxx` to `PIPELIGHT_EXCEPTION:xxx`
- `match_exception()` handles both formats during transition

## Testing Strategy

- Unit tests for `CallbackCommandRegistry`: register, lookup, unknown → Abort
- Unit tests for `ExceptionMapping::resolve()`: stderr marker priority, match_fn fallback, default fallback
- Unit tests for each step's `exception_mapping()` + `match_exception()`
- Integration tests: run pipeline with JSON output, verify new `on_failure` fields
- Existing tests: ensure JSON output remains parseable (backward compat)

## Files Changed

### New Files
- `src/ci/callback/mod.rs`
- `src/ci/callback/action.rs`
- `src/ci/callback/command.rs`
- `src/ci/callback/exception.rs`

### Modified Files
- `src/ci/mod.rs` — add `pub mod callback;`
- `src/ci/pipeline_builder/mod.rs` — StepDef trait + remove on_failure from StepConfig
- `src/ci/pipeline_builder/base/*_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/maven/*_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/gradle/*_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/go/vet_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/node/typecheck_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/python/mypy_step.rs` — implement exception_mapping()
- `src/ci/pipeline_builder/rust_lang/clippy_step.rs` — implement exception_mapping()
- `src/cli/mod.rs` — replace hardcoded match with registry resolution
- `src/run_state/mod.rs` — update OnFailureState fields
- `global-skills/trae-rules/pipelight-run.md` — add Callback Command Reference
- `tests/json_output_test.rs` — update for new JSON fields
- `tests/scenario_test.rs` — update for new retry flow
