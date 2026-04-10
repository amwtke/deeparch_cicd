# on_failure Pipeline Generation + Runtime Resolve + Skill Guardrail

## Problem

1. `pipelight init` generates pipeline.yml with `on_failure: null` for all steps, despite `StepDef::exception_mapping()` defining rich failure strategies
2. Runtime does not call `ExceptionMapping.resolve()` — step failures use the static YAML `callback_command` without analyzing actual stderr/stdout
3. pipelight-run skill executes commands not in pipeline.yml (e.g., running `cargo fmt` directly when `fmt-check` fails)

## Design

### Layer 1: YAML Generation

**Add `ExceptionMapping::to_on_failure()`** in `src/ci/callback/exception.rs`:
- `callback_command` = `self.default_command`
- `max_retries` = max of all entries' `max_retries` (0 if entries empty)
- `context_paths` = deduplicated union of all entries' `context_paths` (empty if entries empty)
- Returns `OnFailure` (imported from `crate::ci::parser`)

**Modify `generate_pipeline()`** in `src/ci/pipeline_builder/mod.rs`:
- After building `all_step_defs` and `all_configs`, call `sd.exception_mapping().to_on_failure()` for each step
- Assign result to the corresponding `Step.on_failure` (as `Some(...)`)
- Remove hardcoded `on_failure: None` from `From<StepConfig> for Step` — instead accept it as a parameter or set it after conversion

### Layer 2: Runtime Exception Resolve

**Modify step failure handling** in `src/cli/mod.rs` (~line 409-487):
- When a step fails (`!result.success`), call `StepDef::exception_mapping().resolve()` with `exit_code`, `stdout`, `stderr`, and `StepDef::match_exception` as `match_fn`
- Use the `ResolvedFailure` result to build `OnFailureState` instead of the static YAML config
- Fallback to YAML `on_failure` if no `StepDef` is available for the step

**Requires**: access to `Vec<Box<dyn StepDef>>` during execution. Currently `generate_pipeline()` returns this, but `cmd_run()` may not retain it when loading from existing YAML. For `pipelight run` (not init), step_defs come from `strategy_for_pipeline()` — need to call `strategy.steps()` to get trait objects.

### Layer 3: Skill Guardrail

**Modify `global-skills/pipelight-run/SKILL.md`**:
- Add rule in "Common Mistakes" and in the retry loop: when a step fails, the skill must ONLY fix code and retry via `pipelight retry`. Never execute pipeline step commands directly (e.g., `cargo fmt`, `mvn compile`) outside the pipeline.
- Rationale: executing commands locally bypasses Docker isolation and the pipeline's own retry/reporting mechanism.

## Affected Files

| File | Change |
|------|--------|
| `src/ci/callback/exception.rs` | Add `to_on_failure()` method |
| `src/ci/pipeline_builder/mod.rs` | Wire `exception_mapping()` into `generate_pipeline()` |
| `src/cli/mod.rs` | Integrate `ExceptionMapping.resolve()` at runtime |
| `global-skills/pipelight-run/SKILL.md` | Add guardrail against direct command execution |

## Testing

- Unit test: `ExceptionMapping::to_on_failure()` returns correct aggregated values
- Unit test: `generate_pipeline()` produces steps with non-None `on_failure`
- Integration: `pipelight init` → verify YAML has `on_failure` fields
- Integration: `pipelight run` with a failing step → verify `ResolvedFailure` is used
