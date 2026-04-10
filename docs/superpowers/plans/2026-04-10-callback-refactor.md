# Callback Command Two-Layer Mapping Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple step exception handling from callback commands and actions via a three-layer model: ExceptionMapping (per-step) → CallbackCommand (global registry) → CallbackCommandAction (Retry/Abort/Notify).

**Architecture:** New `src/ci/callback/` module (zero external deps) defines the action enum, command enum + registry, and exception resolution. StepDef trait gains `exception_mapping()` + `match_exception()`. CLI dispatcher uses registry to resolve actions instead of hardcoded match.

**Tech Stack:** Rust, serde, HashMap, existing pipelight crate structure.

---

### Task 1: Create `callback/action.rs` — CallbackCommandAction enum

**Files:**
- Create: `src/ci/callback/action.rs`

- [ ] **Step 1: Write the test**

In `src/ci/callback/action.rs`, add the enum and inline tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommandAction {
    Retry,
    Abort,
    Notify,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip() {
        for (variant, expected_str) in [
            (CallbackCommandAction::Retry, "\"retry\""),
            (CallbackCommandAction::Abort, "\"abort\""),
            (CallbackCommandAction::Notify, "\"notify\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommandAction = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }
}
```

- [ ] **Step 2: Create mod.rs to wire the module**

Create `src/ci/callback/mod.rs`:

```rust
pub mod action;
```

Add to `src/ci/mod.rs` (line 6, after `pub mod scheduler;`):

```rust
pub mod callback;
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test callback::action --lib`
Expected: PASS — 1 test passes

- [ ] **Step 4: Commit**

```bash
git add src/ci/callback/action.rs src/ci/callback/mod.rs src/ci/mod.rs
git commit -m "feat(callback): add CallbackCommandAction enum"
```

---

### Task 2: Create `callback/command.rs` — CallbackCommand enum + registry

**Files:**
- Create: `src/ci/callback/command.rs`
- Modify: `src/ci/callback/mod.rs`

- [ ] **Step 1: Write command.rs with enum, def, registry, and tests**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::action::CallbackCommandAction;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommand {
    Abort,
    Notify,
    AutoFix,
    AutoGenPmdRuleset,
}

pub struct CallbackCommandDef {
    pub action: CallbackCommandAction,
    pub description: String,
}

pub struct CallbackCommandRegistry {
    commands: HashMap<CallbackCommand, CallbackCommandDef>,
}

impl CallbackCommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };
        registry.register(
            CallbackCommand::Abort,
            CallbackCommandDef {
                action: CallbackCommandAction::Abort,
                description: "Unrecoverable error. Pipeline terminates.".into(),
            },
        );
        registry.register(
            CallbackCommand::Notify,
            CallbackCommandDef {
                action: CallbackCommandAction::Notify,
                description: "Notify user of results. Pipeline terminates.".into(),
            },
        );
        registry.register(
            CallbackCommand::AutoFix,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description: "LLM reads stderr + context_paths, fixes source code, then retries."
                    .into(),
            },
        );
        registry.register(
            CallbackCommand::AutoGenPmdRuleset,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description:
                    "LLM searches project for PMD ruleset or coding guidelines, generates pmd-ruleset.xml, then retries."
                        .into(),
            },
        );
        registry
    }

    pub fn register(&mut self, command: CallbackCommand, def: CallbackCommandDef) {
        self.commands.insert(command, def);
    }

    pub fn get(&self, command: &CallbackCommand) -> Option<&CallbackCommandDef> {
        self.commands.get(command)
    }

    pub fn action_for(&self, command: &CallbackCommand) -> CallbackCommandAction {
        self.commands
            .get(command)
            .map(|def| def.action.clone())
            .unwrap_or(CallbackCommandAction::Abort)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip_all_variants() {
        for (variant, expected_str) in [
            (CallbackCommand::Abort, "\"abort\""),
            (CallbackCommand::Notify, "\"notify\""),
            (CallbackCommand::AutoFix, "\"auto_fix\""),
            (CallbackCommand::AutoGenPmdRuleset, "\"auto_gen_pmd_ruleset\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: CallbackCommand = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn test_registry_built_in_commands() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::Abort),
            CallbackCommandAction::Abort
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::Notify),
            CallbackCommandAction::Notify
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::AutoFix),
            CallbackCommandAction::Retry
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::AutoGenPmdRuleset),
            CallbackCommandAction::Retry
        );
    }

    #[test]
    fn test_registry_get_description() {
        let registry = CallbackCommandRegistry::new();
        let def = registry.get(&CallbackCommand::AutoFix).unwrap();
        assert!(def.description.contains("fixes source code"));
    }

    #[test]
    fn test_registry_all_variants_registered() {
        let registry = CallbackCommandRegistry::new();
        assert!(registry.get(&CallbackCommand::Abort).is_some());
        assert!(registry.get(&CallbackCommand::Notify).is_some());
        assert!(registry.get(&CallbackCommand::AutoFix).is_some());
        assert!(registry.get(&CallbackCommand::AutoGenPmdRuleset).is_some());
    }
}
```

- [ ] **Step 2: Add to mod.rs**

Update `src/ci/callback/mod.rs`:

```rust
pub mod action;
pub mod command;
```

- [ ] **Step 3: Run tests**

Run: `cargo test callback::command --lib`
Expected: PASS — 4 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/ci/callback/command.rs src/ci/callback/mod.rs
git commit -m "feat(callback): add CallbackCommand enum and CallbackCommandRegistry"
```

---

### Task 3: Create `callback/exception.rs` — ExceptionMapping + resolve()

**Files:**
- Create: `src/ci/callback/exception.rs`
- Modify: `src/ci/callback/mod.rs`

- [ ] **Step 1: Write exception.rs with types, resolve logic, and tests**

```rust
use std::collections::HashMap;

use super::command::CallbackCommand;

pub struct ExceptionEntry {
    pub command: CallbackCommand,
    pub max_retries: u32,
    pub context_paths: Vec<String>,
}

pub struct ExceptionMapping {
    entries: HashMap<String, ExceptionEntry>,
    default_command: CallbackCommand,
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
    /// 3. Fallback to default_command
    pub fn resolve(
        &self,
        exit_code: i64,
        stdout: &str,
        stderr: &str,
        match_fn: Option<&dyn Fn(i64, &str, &str) -> Option<String>>,
    ) -> ResolvedFailure {
        let exception_key = Self::parse_stderr_marker(stderr)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_mapping() -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
            .add(
                "ruleset_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenPmdRuleset,
                    max_retries: 2,
                    context_paths: vec!["src/".into()],
                },
            )
            .add(
                "compile_error",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 3,
                    context_paths: vec!["src/".into(), "Cargo.toml".into()],
                },
            )
    }

    #[test]
    fn test_resolve_stderr_marker_priority() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "some output\nPIPELIGHT_EXCEPTION:ruleset_not_found details here\n",
            None,
        );
        assert_eq!(resolved.exception_key, "ruleset_not_found");
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
        assert_eq!(resolved.max_retries, 2);
        assert_eq!(resolved.context_paths, vec!["src/"]);
    }

    #[test]
    fn test_resolve_match_fn_fallback() {
        let mapping = test_mapping();
        let match_fn = |_ec: i64, _out: &str, err: &str| -> Option<String> {
            if err.contains("cannot find value") {
                Some("compile_error".into())
            } else {
                None
            }
        };
        let resolved = mapping.resolve(1, "", "error: cannot find value", Some(&match_fn));
        assert_eq!(resolved.exception_key, "compile_error");
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 3);
    }

    #[test]
    fn test_resolve_stderr_marker_beats_match_fn() {
        let mapping = test_mapping();
        // match_fn would return compile_error, but stderr marker wins
        let match_fn = |_ec: i64, _out: &str, _err: &str| -> Option<String> {
            Some("compile_error".into())
        };
        let resolved = mapping.resolve(
            1,
            "",
            "PIPELIGHT_EXCEPTION:ruleset_not_found\nerror: cannot find value",
            Some(&match_fn),
        );
        assert_eq!(resolved.exception_key, "ruleset_not_found");
        assert_eq!(resolved.command, CallbackCommand::AutoGenPmdRuleset);
    }

    #[test]
    fn test_resolve_default_fallback() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(1, "", "some unknown error", None);
        assert_eq!(resolved.exception_key, "unrecognized");
        assert_eq!(resolved.command, CallbackCommand::Abort);
        assert_eq!(resolved.max_retries, 0);
        assert!(resolved.context_paths.is_empty());
    }

    #[test]
    fn test_resolve_unmapped_exception_key() {
        let mapping = test_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "PIPELIGHT_EXCEPTION:totally_unknown_key\n",
            None,
        );
        // Key found in stderr but not in mapping → fallback to default
        assert_eq!(resolved.exception_key, "totally_unknown_key");
        assert_eq!(resolved.command, CallbackCommand::Abort);
        assert_eq!(resolved.max_retries, 0);
    }

    #[test]
    fn test_parse_stderr_marker_first_match() {
        let stderr = "line1\nPIPELIGHT_EXCEPTION:first_key\nPIPELIGHT_EXCEPTION:second_key\n";
        let key = ExceptionMapping::parse_stderr_marker(stderr);
        assert_eq!(key, Some("first_key".into()));
    }

    #[test]
    fn test_parse_stderr_marker_empty_key_ignored() {
        let stderr = "PIPELIGHT_EXCEPTION: \nPIPELIGHT_EXCEPTION:valid_key\n";
        let key = ExceptionMapping::parse_stderr_marker(stderr);
        assert_eq!(key, Some("valid_key".into()));
    }

    #[test]
    fn test_parse_stderr_marker_none_when_absent() {
        let key = ExceptionMapping::parse_stderr_marker("just some error output\n");
        assert!(key.is_none());
    }
}
```

- [ ] **Step 2: Add to mod.rs**

Update `src/ci/callback/mod.rs`:

```rust
pub mod action;
pub mod command;
pub mod exception;
```

- [ ] **Step 3: Run tests**

Run: `cargo test callback::exception --lib`
Expected: PASS — 8 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/ci/callback/exception.rs src/ci/callback/mod.rs
git commit -m "feat(callback): add ExceptionMapping with three-priority resolve chain"
```

---

### Task 4: Migrate parser to use canonical CallbackCommand enum

**Files:**
- Modify: `src/ci/parser/mod.rs:92-115`
- Modify: all files that `use crate::ci::parser::{CallbackCommand, OnFailure}`

- [ ] **Step 1: Delete parser's CallbackCommand enum, re-export from callback**

In `src/ci/parser/mod.rs`, replace lines 92-115:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackCommand {
    Abort,
    AutoFix,
    AutoGenPmdRuleset,
    Notify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailure {
    #[serde(default = "default_callback_command")]
    pub callback_command: CallbackCommand,

    #[serde(default)]
    pub max_retries: u32,

    #[serde(default)]
    pub context_paths: Vec<String>,
}

fn default_callback_command() -> CallbackCommand {
    CallbackCommand::Abort
}
```

With:

```rust
// Re-export the canonical CallbackCommand from callback module
pub use crate::ci::callback::command::CallbackCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailure {
    #[serde(default = "default_callback_command")]
    pub callback_command: CallbackCommand,

    #[serde(default)]
    pub max_retries: u32,

    #[serde(default)]
    pub context_paths: Vec<String>,
}

fn default_callback_command() -> CallbackCommand {
    CallbackCommand::Abort
}
```

- [ ] **Step 2: Run all existing tests to confirm nothing breaks**

Run: `cargo test`
Expected: All existing tests PASS. The re-exported enum has identical variants and serde behavior, so all `use crate::ci::parser::CallbackCommand` imports continue to work without changes.

- [ ] **Step 3: Commit**

```bash
git add src/ci/parser/mod.rs
git commit -m "refactor(parser): re-export CallbackCommand from callback module"
```

---

### Task 5: Add `exception_mapping()` + `match_exception()` to StepDef trait

**Files:**
- Modify: `src/ci/pipeline_builder/mod.rs:14,24,38,54,66-81`

- [ ] **Step 1: Add default methods to StepDef trait and update imports**

In `src/ci/pipeline_builder/mod.rs`, add import at line 14 (alongside existing imports):

```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::ExceptionMapping;
```

Add two default methods to the `StepDef` trait (after `output_report_path`, around line 81):

```rust
    /// Return the exception-to-command mapping for this step.
    /// Default: empty mapping with Abort fallback (all failures are fatal).
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }

    /// Analyze execution output to identify the exception key.
    /// Called as priority 2 in resolve chain (after stderr marker).
    /// Default: None (no Rust-side analysis).
    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        None
    }
```

- [ ] **Step 2: Run tests to verify defaults don't break existing steps**

Run: `cargo test`
Expected: All PASS. Default methods mean existing StepDef impls don't need changes yet.

- [ ] **Step 3: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs
git commit -m "feat(pipeline_builder): add exception_mapping() and match_exception() defaults to StepDef"
```

---

### Task 6: Migrate base steps (git_pull, build, test, lint, fmt)

**Files:**
- Modify: `src/ci/pipeline_builder/base/git_pull_step.rs`
- Modify: `src/ci/pipeline_builder/base/build_step.rs`
- Modify: `src/ci/pipeline_builder/base/test_step.rs`
- Modify: `src/ci/pipeline_builder/base/lint_step.rs`
- Modify: `src/ci/pipeline_builder/base/fmt_step.rs`

Each step: add `exception_mapping()` override, remove `on_failure` from `config()`, update tests.

- [ ] **Step 1: Migrate git_pull_step.rs**

Replace imports:
```rust
// BEFORE
use crate::ci::parser::{CallbackCommand, OnFailure};
// AFTER
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionMapping, ExceptionEntry};
```

In `config()`, remove the `on_failure` field (lines 25-29):
```rust
// Remove this from config():
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::Abort,
                max_retries: 0,
                context_paths: vec![],
            }),
```

Add `exception_mapping()` to the impl:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }
```

Update test `test_config` — remove `on_failure` assertions, add separate test:
```rust
    #[test]
    fn test_config() {
        let step = GitPullStep::new();
        let cfg = step.config();
        assert_eq!(cfg.name, "git-pull");
        assert!(cfg.local);
        assert!(cfg.image.is_empty());
        assert!(cfg.depends_on.is_empty());
        assert!(cfg.volumes.is_empty());
    }

    #[test]
    fn test_exception_mapping() {
        let step = GitPullStep::new();
        let resolved = step.exception_mapping().resolve(1, "", "some error", None);
        assert_eq!(resolved.command, CallbackCommand::Abort);
        assert_eq!(resolved.max_retries, 0);
    }
```

- [ ] **Step 2: Migrate build_step.rs**

Replace imports same pattern. Remove `on_failure` from `config()` (lines 29-33). Add:

```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("compile_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 3,
                context_paths: [&self.source_paths[..], &self.config_files[..]].concat(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("compile_error".into())
    }
```

Update test:
```rust
    #[test]
    fn test_exception_mapping() {
        let step = BuildStep::new(&make_info());
        let resolved = step.exception_mapping().resolve(
            1, "", "error: cannot find value",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 3);
        assert!(resolved.context_paths.contains(&"src/".to_string()));
        assert!(resolved.context_paths.contains(&"Cargo.toml".to_string()));
    }
```

- [ ] **Step 3: Migrate test_step.rs**

Remove `on_failure` from `config()`. Add:

```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Notify)
    }
```

Update test:
```rust
    #[test]
    fn test_exception_mapping() {
        let step = TestStep::new(&make_info());
        let resolved = step.exception_mapping().resolve(1, "", "assertion failed", None);
        assert_eq!(resolved.command, CallbackCommand::Notify);
        assert_eq!(resolved.max_retries, 0);
    }
```

- [ ] **Step 4: Migrate lint_step.rs**

Remove `on_failure` from `config()`. Add:

```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("lint_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("lint_error".into())
    }
```

Update test:
```rust
    #[test]
    fn test_exception_mapping() {
        let mut info = make_info();
        info.lint_cmd = Some(vec!["cargo clippy".into()]);
        let step = LintStep::new(&info).unwrap();
        let resolved = step.exception_mapping().resolve(
            1, "", "warning turned error",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 2);
    }
```

- [ ] **Step 5: Migrate fmt_step.rs**

Remove `on_failure` from `config()`. Add:

```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("fmt_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 1,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("fmt_error".into())
    }
```

Update test:
```rust
    #[test]
    fn test_exception_mapping() {
        let mut info = make_info();
        info.fmt_cmd = Some(vec!["cargo fmt -- --check".into()]);
        let step = FmtStep::new(&info).unwrap();
        let resolved = step.exception_mapping().resolve(
            1, "", "diff in foo.rs",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 1);
    }
```

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src/ci/pipeline_builder/base/
git commit -m "refactor(base_steps): migrate 5 base steps to exception_mapping()"
```

---

### Task 7: Migrate language-specific steps

**Files:**
- Modify: `src/ci/pipeline_builder/maven/pmd_step.rs`
- Modify: `src/ci/pipeline_builder/maven/checkstyle_step.rs`
- Modify: `src/ci/pipeline_builder/maven/spotbugs_step.rs`
- Modify: `src/ci/pipeline_builder/maven/package_step.rs`
- Modify: `src/ci/pipeline_builder/gradle/pmd_step.rs`
- Modify: `src/ci/pipeline_builder/gradle/checkstyle_step.rs`
- Modify: `src/ci/pipeline_builder/gradle/spotbugs_step.rs`
- Modify: `src/ci/pipeline_builder/go/vet_step.rs`
- Modify: `src/ci/pipeline_builder/node/typecheck_step.rs`
- Modify: `src/ci/pipeline_builder/python/mypy_step.rs`
- Modify: `src/ci/pipeline_builder/rust_lang/clippy_step.rs`

Same pattern for all: replace imports, remove `on_failure` from `config()`, add `exception_mapping()` + optional `match_exception()`, update tests.

- [ ] **Step 1: Migrate maven/pmd_step.rs (most complex — validates design)**

Replace imports:
```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
```

Remove `on_failure` from `config()` (lines 88-93).

Add:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
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
            Some("ruleset_not_found".into())
        } else {
            None
        }
    }
```

- [ ] **Step 2: Migrate maven/checkstyle_step.rs**

Remove `on_failure`, add:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("checkstyle_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.config_files.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("checkstyle_error".into())
    }
```

- [ ] **Step 3: Migrate maven/spotbugs_step.rs**

Remove `on_failure`, add:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("spotbugs_found", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("spotbugs_found".into())
    }
```

- [ ] **Step 4: Migrate maven/package_step.rs**

Remove `on_failure`, add:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }
```

- [ ] **Step 5: Migrate gradle/pmd_step.rs, gradle/checkstyle_step.rs, gradle/spotbugs_step.rs**

Same pattern as their Maven counterparts. Replace imports, remove `on_failure`, add `exception_mapping()` + `match_exception()`.

Gradle PMD uses same exception keys as Maven PMD (`ruleset_not_found`, `ruleset_invalid`).
Gradle checkstyle/spotbugs use same simple pattern as Maven counterparts.

- [ ] **Step 6: Migrate go/vet_step.rs, node/typecheck_step.rs, python/mypy_step.rs, rust_lang/clippy_step.rs**

All follow the simple AutoFix pattern:
```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix)
            .add("<step>_error", ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            })
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("<step>_error".into())
    }
```

Where `<step>` is `vet`, `typecheck`, `mypy`, `clippy` respectively.

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add src/ci/pipeline_builder/maven/ src/ci/pipeline_builder/gradle/ src/ci/pipeline_builder/go/ src/ci/pipeline_builder/node/ src/ci/pipeline_builder/python/ src/ci/pipeline_builder/rust_lang/
git commit -m "refactor(steps): migrate 11 language-specific steps to exception_mapping()"
```

---

### Task 8: Remove `on_failure` from StepConfig and update From impl

**Files:**
- Modify: `src/ci/pipeline_builder/mod.rs:14,24,38,46-61`

- [ ] **Step 1: Remove on_failure from StepConfig**

In `src/ci/pipeline_builder/mod.rs`:

Remove `use crate::ci::parser::{OnFailure, Pipeline, Step};` → change to `use crate::ci::parser::{Pipeline, Step};`

Remove from `StepConfig` struct (line 24):
```rust
    pub on_failure: Option<OnFailure>,
```

Remove from `Default` impl (line 38):
```rust
            on_failure: None,
```

Remove from `From<StepConfig> for Step` impl (line 54):
```rust
            on_failure: sc.on_failure,
```

The `Step` struct in parser still has `on_failure` for YAML compat — set it to `None` in the From impl:
```rust
            on_failure: None,
```

- [ ] **Step 2: Fix any remaining compilation errors**

Check that no step's `config()` still references `on_failure`. All were removed in Tasks 6-7.

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs
git commit -m "refactor(pipeline_builder): remove on_failure from StepConfig"
```

---

### Task 9: Update OnFailureState and CLI dispatcher

**Files:**
- Modify: `src/run_state/mod.rs:33-39`
- Modify: `src/cli/mod.rs:9,406-484,519-525,1083-1105`

- [ ] **Step 1: Update OnFailureState in run_state**

In `src/run_state/mod.rs`, add imports and update `OnFailureState`:

```rust
use crate::ci::callback::action::CallbackCommandAction;
use crate::ci::callback::command::CallbackCommand;
```

Replace `OnFailureState` (lines 33-39):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailureState {
    pub exception_key: String,
    pub command: CallbackCommand,
    pub action: CallbackCommandAction,
    pub max_retries: u32,
    pub retries_remaining: u32,
    pub context_paths: Vec<String>,
}
```

- [ ] **Step 2: Update CLI dispatcher — on_failure_state construction**

In `src/cli/mod.rs`, add imports:
```rust
use crate::ci::callback::action::CallbackCommandAction;
use crate::ci::callback::command::CallbackCommandRegistry;
```

Replace the `on_failure_state` construction (lines 406-421). Instead of building from `pipeline_step.on_failure`, resolve via exception_mapping:

```rust
            // Resolve on_failure via exception_mapping
            let on_failure_state = if !result.success {
                // Find the step_def for this step
                if let Some(sd) = step_defs.iter().find(|sd| sd.config().name == *step_name) {
                    let resolved = sd.exception_mapping().resolve(
                        result.exit_code,
                        &stdout,
                        &stderr,
                        Some(&|ec, out, err| sd.match_exception(ec, out, err)),
                    );
                    let action = registry.action_for(&resolved.command);
                    Some(OnFailureState {
                        exception_key: resolved.exception_key,
                        command: resolved.command,
                        action: action.clone(),
                        max_retries: resolved.max_retries,
                        retries_remaining: resolved.max_retries,
                        context_paths: resolved.context_paths,
                    })
                } else {
                    None
                }
            } else {
                None
            };
```

Note: `registry` and `step_defs` must be available in scope. `registry` is created at the start of `cmd_run` via `let registry = CallbackCommandRegistry::new();`. `step_defs` is already available from `generate_pipeline()`.

- [ ] **Step 3: Update CLI dispatcher — failure classification**

Replace the hardcoded match (lines 470-484):

```rust
            if !result.success && !allow_failure {
                if let Some(ref ofs) = on_failure_state {
                    match ofs.action {
                        CallbackCommandAction::Retry if ofs.max_retries > 0 => {
                            has_retryable_failure = true;
                            break 'outer;
                        }
                        _ => {
                            has_final_failure = true;
                            break 'outer;
                        }
                    }
                } else {
                    has_final_failure = true;
                    break 'outer;
                }
            }
```

- [ ] **Step 4: Remove old CallbackCommand import from cli/mod.rs**

Line 9: change `use crate::ci::parser::{CallbackCommand, Pipeline};` to `use crate::ci::parser::Pipeline;`

- [ ] **Step 5: Delete the `test_strategy_to_string_all_variants` test**

This test (lines 1083-1105) tested the old manual match-based serialization. The enum now uses serde, and this is already tested in `callback::command::tests::test_serde_roundtrip_all_variants`.

- [ ] **Step 6: Update run_state tests**

Update all `OnFailureState` constructions in `src/run_state/mod.rs` tests to use the new fields. For example, the test at line 203:

```rust
            on_failure: Some(OnFailureState {
                exception_key: "compile_error".into(),
                command: CallbackCommand::AutoFix,
                action: CallbackCommandAction::Retry,
                max_retries: 3,
                retries_remaining: 3,
                context_paths: vec!["src/".into()],
            }),
```

Apply same pattern to all other `OnFailureState` constructions in the test module.

- [ ] **Step 7: Run tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add src/run_state/mod.rs src/cli/mod.rs
git commit -m "refactor(cli): replace hardcoded callback match with registry-based resolution"
```

---

### Task 10: Update JSON output tests

**Files:**
- Modify: `tests/json_output_test.rs`

- [ ] **Step 1: Update on_failure JSON assertions**

In `tests/json_output_test.rs`, update all `on_failure` JSON objects in test fixtures from:
```json
"on_failure": {
    "callback_command": "auto_fix",
    "max_retries": 3,
    "retries_remaining": 2,
    "context_paths": ["src/"]
}
```

To the new format:
```json
"on_failure": {
    "exception_key": "compile_error",
    "command": "auto_fix",
    "action": "retry",
    "max_retries": 3,
    "retries_remaining": 2,
    "context_paths": ["src/"]
}
```

Update assertions from `of["callback_command"]` to `of["command"]` and add `of["action"]` and `of["exception_key"]` checks.

Affected tests:
- `json_status_failed_pipeline_with_on_failure` (line 186-238)
- `json_status_retryable_pipeline` (line 251-293)
- `json_real_execution_retryable` (line 586-604)

- [ ] **Step 2: Run tests**

Run: `cargo test --test json_output_test`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add tests/json_output_test.rs
git commit -m "test: update JSON output tests for new on_failure format"
```

---

### Task 11: Update pipelight-run skill with Callback Command Reference

**Files:**
- Modify: `global-skills/trae-rules/pipelight-run.md`

- [ ] **Step 1: Add Callback Command Reference section**

After the "Exit Code Reference" section (around line 140), add:

```markdown
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
```

- [ ] **Step 2: Update Step 4 flow to reference the mapping table**

Replace the current Step 4 section (lines 104-133) with:

```markdown
## Step 4: Act on Status

### `status: "success"`

Report success to user. Show step durations if relevant.

### `status: "failed"`

The failed step's `on_failure.action` is `abort` or `notify`. Report the error:
- Show which step failed
- Show `stderr` content
- Show `error_context` if present
- Do NOT attempt auto-fix

### `status: "retryable"`

The failed step's `on_failure.action` is `retry`. Enter the fix-retry loop:

1. Find the failed step (the one with `status: "failed"`)
2. Read `on_failure.command` — look it up in the **Command Mapping Table** above
3. Execute the command's **LLM reasoning instructions**
4. Check `retries_remaining > 0` before retrying
5. Run retry:

\```bash
pipelight retry --run-id <same-run-id> --step <failed-step-name> -f pipeline.yml --output json
\```

6. Parse the new JSON result and repeat from Step 4
```

- [ ] **Step 3: Update JSON structure example**

Replace the old `on_failure` example (lines 92-99) with:
```json
"on_failure": {
    "exception_key": "compile_error",
    "command": "auto_fix",
    "action": "retry",
    "max_retries": 3,
    "retries_remaining": 3,
    "context_paths": ["src/", "Cargo.toml"]
}
```

- [ ] **Step 4: Sync to local skills**

```bash
cp -r global-skills/trae-rules/ ~/.claude/skills/trae-rules/
```

- [ ] **Step 5: Commit**

```bash
git add global-skills/trae-rules/pipelight-run.md
git commit -m "docs(skill): add Callback Command Reference to pipelight-run skill"
```

---

### Task 12: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Verify JSON output format manually**

Run: `cargo run -- status --run-id <any-existing-id> --output json 2>/dev/null || true`
Verify the `on_failure` fields contain `exception_key`, `command`, and `action`.

- [ ] **Step 4: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore: final cleanup for callback refactor"
```
