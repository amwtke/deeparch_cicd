# on_failure Pipeline Generation + Runtime Resolve + Skill Guardrail

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `pipelight init` write `on_failure` into pipeline.yml from StepDef exception_mapping, wire runtime exception resolution, and add skill guardrail against direct command execution.

**Architecture:** Layer 1 adds `ExceptionMapping::to_on_failure()` and wires it into `generate_pipeline()`. Layer 2 adds runtime resolve by reconstructing step_defs from the pipeline strategy and calling `ExceptionMapping::resolve()` on failure. Layer 3 updates the pipelight-run skill doc.

**Tech Stack:** Rust (serde, anyhow), Markdown (skill doc)

---

### Task 1: Add `ExceptionMapping::to_on_failure()` method

**Files:**
- Modify: `src/ci/callback/exception.rs:12-16` (ExceptionMapping impl)

- [ ] **Step 1: Write the failing test**

Add to the existing `tests` module in `src/ci/callback/exception.rs`:

```rust
#[test]
fn test_to_on_failure_with_entries() {
    let mapping = test_mapping(); // default=RuntimeError, entries: ruleset_not_found(retries=2, paths=["src/"]), compile_error(retries=3, paths=["src/","Cargo.toml"])
    let of = mapping.to_on_failure();
    assert_eq!(of.callback_command, CallbackCommand::RuntimeError);
    assert_eq!(of.max_retries, 3); // max of 2 and 3
    assert!(of.context_paths.contains(&"src/".to_string()));
    assert!(of.context_paths.contains(&"Cargo.toml".to_string()));
    assert_eq!(of.context_paths.len(), 2); // deduplicated: "src/" appears in both entries
}

#[test]
fn test_to_on_failure_empty_entries() {
    let mapping = ExceptionMapping::new(CallbackCommand::Abort);
    let of = mapping.to_on_failure();
    assert_eq!(of.callback_command, CallbackCommand::Abort);
    assert_eq!(of.max_retries, 0);
    assert!(of.context_paths.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pipelight test_to_on_failure -- --nocapture`
Expected: FAIL — `to_on_failure` method does not exist

- [ ] **Step 3: Implement `to_on_failure()`**

Add to `impl ExceptionMapping` in `src/ci/callback/exception.rs`, after the existing `resolve()` method (around line 76):

```rust
/// Convert this mapping's aggregate info into an OnFailure for YAML serialization.
/// - callback_command = default_command
/// - max_retries = max of all entries' max_retries (0 if no entries)
/// - context_paths = deduplicated union of all entries' context_paths
pub fn to_on_failure(&self) -> crate::ci::parser::OnFailure {
    let max_retries = self.entries.values().map(|e| e.max_retries).max().unwrap_or(0);

    let mut paths: Vec<String> = self
        .entries
        .values()
        .flat_map(|e| e.context_paths.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();

    crate::ci::parser::OnFailure {
        callback_command: self.default_command.clone(),
        max_retries,
        context_paths: paths,
    }
}
```

Remove the `#[allow(dead_code)]` from the `ExceptionMapping` struct, `new()`, and `add()` methods since they are now used by `to_on_failure()` in production code.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pipelight test_to_on_failure -- --nocapture`
Expected: PASS — both tests pass

- [ ] **Step 5: Commit**

```bash
git add src/ci/callback/exception.rs
git commit -m "feat(exception): add to_on_failure() to aggregate ExceptionMapping into OnFailure"
```

---

### Task 2: Wire `to_on_failure()` into `generate_pipeline()`

**Files:**
- Modify: `src/ci/pipeline_builder/mod.rs:162-209` (generate_pipeline function)

- [ ] **Step 1: Write the failing test**

Add a new test in `src/ci/pipeline_builder/mod.rs` (at the bottom, create a `#[cfg(test)] mod tests` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::command::CallbackCommand;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_rust_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("2021".into()),
            framework: None,
            image: "rust:latest".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".into(),
            ]),
            fmt_cmd: Some(vec![
                "rustup component add rustfmt 2>/dev/null; cargo fmt -- --check".into(),
            ]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_generate_pipeline_has_on_failure() {
        let info = make_rust_info();
        let (pipeline, _step_defs) = generate_pipeline(&info);

        // git-pull: RuntimeError, no retries
        let git_pull = pipeline.get_step("git-pull").unwrap();
        let of = git_pull.on_failure.as_ref().expect("git-pull should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::RuntimeError);
        assert_eq!(of.max_retries, 0);

        // build: AutoFix, 3 retries
        let build = pipeline.get_step("build").unwrap();
        let of = build.on_failure.as_ref().expect("build should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 3);
        assert!(of.context_paths.contains(&"src/".to_string()));
        assert!(of.context_paths.contains(&"Cargo.toml".to_string()));

        // test: Abort, no retries
        let test = pipeline.get_step("test").unwrap();
        let of = test.on_failure.as_ref().expect("test should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::Abort);
        assert_eq!(of.max_retries, 0);

        // fmt-check: AutoFix, 1 retry
        let fmt = pipeline.get_step("fmt-check").unwrap();
        let of = fmt.on_failure.as_ref().expect("fmt-check should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 1);
        assert!(of.context_paths.contains(&"src/".to_string()));

        // clippy: AutoFix, 2 retries
        let clippy = pipeline.get_step("clippy").unwrap();
        let of = clippy.on_failure.as_ref().expect("clippy should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pipelight test_generate_pipeline_has_on_failure -- --nocapture`
Expected: FAIL — all `on_failure` fields are `None`

- [ ] **Step 3: Modify `generate_pipeline()` to wire on_failure**

In `src/ci/pipeline_builder/mod.rs`, replace the pipeline construction (lines 198-206) with:

```rust
    // Convert configs to Steps, then attach on_failure from exception_mapping
    let mut steps: Vec<Step> = all_configs.into_iter().map(|sc| sc.into()).collect();

    // Attach on_failure from each StepDef's exception_mapping
    for (step, sd) in steps.iter_mut().zip(all_step_defs.iter()) {
        step.on_failure = Some(sd.exception_mapping().to_on_failure());
    }

    let pipeline = Pipeline {
        name,
        git_credentials: Some(crate::ci::parser::GitCredentials {
            username: "your_username".to_string(),
            password: "your_token_or_password".to_string(),
        }),
        env: HashMap::new(),
        steps,
    };

    (pipeline, all_step_defs)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pipelight test_generate_pipeline_has_on_failure -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All existing tests pass (some existing tests may check `on_failure.is_none()` — if so, update them to match new behavior)

- [ ] **Step 6: Integration verification**

```bash
pipelight clean 2>/dev/null; pipelight init -d . && cat pipeline.yml
```

Expected: `pipeline.yml` now contains `on_failure` blocks with `callback_command`, `max_retries`, and `context_paths` for each step.

- [ ] **Step 7: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs
git commit -m "feat(pipeline): wire StepDef exception_mapping into generated pipeline.yml on_failure"
```

---

### Task 3: Runtime exception resolve on step failure

**Files:**
- Modify: `src/ci/cli/mod.rs:409-423` (on_failure_state construction)
- Modify: `src/ci/pipeline_builder/mod.rs` (add `step_defs_for_pipeline()` helper)

- [ ] **Step 1: Add `step_defs_for_pipeline()` helper**

At runtime (`pipelight run`), the step_defs aren't available because the pipeline was loaded from YAML, not generated. We need a way to reconstruct them. Add to `src/ci/pipeline_builder/mod.rs`, after `strategy_for_pipeline()`:

```rust
/// Reconstruct StepDef trait objects for a pipeline loaded from YAML.
/// Returns a map from step name to StepDef. Only works if the pipeline
/// was generated by pipelight (name matches a known strategy).
/// Returns None if no matching strategy is found.
pub fn step_defs_for_pipeline(pipeline: &Pipeline) -> Option<HashMap<String, Box<dyn StepDef>>> {
    let strategy = strategy_for_pipeline(pipeline)?;

    // We need ProjectInfo to build step defs, but we don't have it when loading from YAML.
    // Reconstruct a minimal ProjectInfo from the pipeline steps.
    let first_docker_step = pipeline.steps.iter().find(|s| !s.image.is_empty())?;
    let source_paths: Vec<String> = pipeline
        .steps
        .iter()
        .filter_map(|s| s.on_failure.as_ref())
        .flat_map(|of| of.context_paths.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let info = crate::ci::detector::ProjectInfo {
        project_type: match pipeline.name.as_str() {
            n if n.starts_with("maven") => crate::ci::detector::ProjectType::Maven,
            n if n.starts_with("gradle") => crate::ci::detector::ProjectType::Gradle,
            n if n.starts_with("rust") => crate::ci::detector::ProjectType::Rust,
            n if n.starts_with("node") => crate::ci::detector::ProjectType::Node,
            n if n.starts_with("python") => crate::ci::detector::ProjectType::Python,
            n if n.starts_with("go") => crate::ci::detector::ProjectType::Go,
            _ => return None,
        },
        language_version: None,
        framework: None,
        image: first_docker_step.image.clone(),
        build_cmd: pipeline
            .get_step("build")
            .map(|s| s.commands.clone())
            .unwrap_or_default(),
        test_cmd: pipeline
            .get_step("test")
            .map(|s| s.commands.clone())
            .unwrap_or_default(),
        lint_cmd: pipeline
            .get_step("clippy")
            .or_else(|| pipeline.get_step("lint"))
            .map(|s| s.commands.clone()),
        fmt_cmd: pipeline
            .get_step("fmt-check")
            .map(|s| s.commands.clone()),
        source_paths,
        config_files: vec![],
        warnings: vec![],
        quality_plugins: vec![],
        subdir: None,
    };

    let step_defs = strategy.steps(&info);
    let git_pull = base::GitPullStep::new();

    let mut map: HashMap<String, Box<dyn StepDef>> = HashMap::new();
    map.insert(git_pull.config().name, Box::new(git_pull));
    for sd in step_defs {
        map.insert(sd.config().name, sd);
    }
    Some(map)
}
```

- [ ] **Step 2: Write the failing test for runtime resolve**

Add a test in `src/ci/pipeline_builder/mod.rs` tests module:

```rust
#[test]
fn test_step_defs_for_pipeline_rust() {
    let info = make_rust_info();
    let (pipeline, _) = generate_pipeline(&info);
    let defs = step_defs_for_pipeline(&pipeline).expect("should find rust strategy");
    assert!(defs.contains_key("git-pull"));
    assert!(defs.contains_key("build"));
    assert!(defs.contains_key("test"));
    assert!(defs.contains_key("clippy"));
    assert!(defs.contains_key("fmt-check"));
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p pipelight test_step_defs_for_pipeline_rust -- --nocapture`
Expected: PASS (the implementation was added in Step 1)

- [ ] **Step 4: Integrate resolve into `cmd_run()`**

In `src/cli/mod.rs`, after `let registry = CallbackCommandRegistry::new();` (line 203), add:

```rust
let step_def_map = crate::ci::pipeline_builder::step_defs_for_pipeline(&pipeline);
```

Then replace the `on_failure_state` construction block (lines 409-423) with:

```rust
            // Build on_failure state: prefer runtime exception resolve, fall back to YAML config
            let on_failure_state = if !result.success {
                // Try runtime resolve via StepDef exception_mapping
                if let Some(ref defs) = step_def_map {
                    if let Some(sd) = defs.get(step_name) {
                        let mapping = sd.exception_mapping();
                        let match_fn = |ec: i64, out: &str, err: &str| -> Option<String> {
                            sd.match_exception(ec, out, err)
                        };
                        let resolved = mapping.resolve(
                            result.exit_code as i64,
                            &stdout,
                            &stderr,
                            Some(&match_fn),
                        );
                        let action = registry.action_for(&resolved.command);
                        Some(OnFailureState {
                            exception_key: resolved.exception_key,
                            command: resolved.command,
                            action,
                            max_retries: resolved.max_retries,
                            retries_remaining: resolved.max_retries,
                            context_paths: resolved.context_paths,
                        })
                    } else {
                        // No StepDef for this step, fall back to YAML
                        pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                            let action = registry.action_for(&of.callback_command);
                            OnFailureState {
                                exception_key: "yaml_configured".into(),
                                command: of.callback_command.clone(),
                                action,
                                max_retries: of.max_retries,
                                retries_remaining: of.max_retries,
                                context_paths: of.context_paths.clone(),
                            }
                        })
                    }
                } else {
                    // No strategy found, fall back to YAML
                    pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                        let action = registry.action_for(&of.callback_command);
                        OnFailureState {
                            exception_key: "yaml_configured".into(),
                            command: of.callback_command.clone(),
                            action,
                            max_retries: of.max_retries,
                            retries_remaining: of.max_retries,
                            context_paths: of.context_paths.clone(),
                        }
                    })
                }
            } else {
                // Step succeeded — still attach on_failure for JSON output completeness
                pipeline_step.and_then(|s| s.on_failure.as_ref()).map(|of| {
                    let action = registry.action_for(&of.callback_command);
                    OnFailureState {
                        exception_key: "yaml_configured".into(),
                        command: of.callback_command.clone(),
                        action,
                        max_retries: of.max_retries,
                        retries_remaining: of.max_retries,
                        context_paths: of.context_paths.clone(),
                    }
                })
            };
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs src/cli/mod.rs
git commit -m "feat(runtime): resolve exception_mapping at runtime for dynamic on_failure"
```

---

### Task 4: Skill guardrail — prohibit direct command execution

**Files:**
- Modify: `global-skills/pipelight-run/SKILL.md`

- [ ] **Step 1: Add guardrail section**

In `global-skills/pipelight-run/SKILL.md`, add a new `## Guardrails` section before `## Exit Code Reference`:

```markdown
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
```

- [ ] **Step 2: Add to Common Mistakes table**

Add this row to the existing "Common Mistakes" table:

```markdown
| Execute step commands directly (e.g., `cargo fmt`) | Only fix source code and retry via `pipelight retry`. Never run step commands outside the pipeline |
```

- [ ] **Step 3: Sync to local skills**

```bash
cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/
```

- [ ] **Step 4: Commit**

```bash
git add global-skills/pipelight-run/SKILL.md
git commit -m "docs(skill): add guardrail against direct command execution in pipelight-run"
```

---

### Task 5: End-to-end verification

**Files:** None (verification only)

- [ ] **Step 1: Clean and reinit**

```bash
pipelight clean 2>/dev/null; pipelight init -d .
```

Verify `pipeline.yml` has `on_failure` blocks for all steps.

- [ ] **Step 2: Run pipeline**

```bash
pipelight run -f pipeline.yml --output json --run-id verify-001
```

Verify JSON output has populated `on_failure` fields with correct `strategy`, `max_retries`, and `context_paths`.

- [ ] **Step 3: Run full test suite**

```bash
cargo test
```

Expected: All tests pass.

- [ ] **Step 4: Clean up generated files**

```bash
pipelight clean
```

- [ ] **Step 5: Final commit (if any fixups needed)**

```bash
git add -A && git commit -m "fix: end-to-end verification fixups"
```
