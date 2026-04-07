# StepDef Trait Refactoring + Directory Rename

## Goal

Refactor `StepDef` from a plain data struct to a trait (interface), so each pipeline step is a self-contained unit with its own config, report logic, and log writing. Rename `builder/` to `pipeline_builder/` for clarity.

## Current State

- `StepDef` is a struct (DTO) — no behavior, just data fields
- Report logic lives in `PipelineStrategy::output_report_str()`, dispatching by step name string matching
- Step files (e.g., `checkstyle.rs`) export a function `pub fn step(info) -> StepDef`
- Directory is `src/ci/builder/`

## Target State

### 1. StepDef becomes a trait

```rust
/// Pure data — the old StepDef renamed
pub struct StepConfig {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
    pub on_failure: Option<OnFailure>,
    pub allow_failure: bool,
    pub volumes: Vec<String>,
}

/// Every pipeline step implements this trait
pub trait StepDef: Send + Sync {
    /// Return the step's static configuration
    fn config(&self) -> StepConfig;

    /// Parse stdout/stderr into a one-line summary
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String;

    /// Write stdout/stderr to a timestamped log file, return the path.
    /// Default implementation: pipelight-misc/{name}-{yyyyMMddTHHmmss}.log
    fn output_report_path(&self, misc_dir: &Path, stdout: &str, stderr: &str) -> PathBuf {
        write_step_report(misc_dir, &self.config().name, stdout, stderr)
    }
}
```

### 2. Each step is a struct implementing StepDef

Each step file becomes a struct + `impl StepDef`. The struct stores only what it needs from `ProjectInfo` at construction time.

Example:
```rust
// maven/checkstyle_step.rs
pub struct CheckstyleStep {
    image: String,
    config_files: Vec<String>,
    subdir: Option<String>,
}

impl CheckstyleStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            config_files: info.config_files.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for CheckstyleStep {
    fn config(&self) -> StepConfig { /* ... */ }
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        // checkstyle-specific violation counting
    }
}
```

### 3. PipelineStrategy simplified

```rust
pub trait PipelineStrategy {
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>>;
}
```

- `output_report_str` removed from strategy (now on each step)
- `parse_test_output` removed from strategy (now inside TestStep)

### 4. File renames

Step files get `_step` suffix:

| Before | After |
|--------|-------|
| `maven/checkstyle.rs` | `maven/checkstyle_step.rs` |
| `maven/package.rs` | `maven/package_step.rs` |
| `maven/pmd.rs` | `maven/pmd_step.rs` |
| `maven/spotbugs.rs` | `maven/spotbugs_step.rs` |
| `gradle/checkstyle.rs` | `gradle/checkstyle_step.rs` |
| `gradle/pmd.rs` | `gradle/pmd_step.rs` |
| `gradle/spotbugs.rs` | `gradle/spotbugs_step.rs` |
| `rust_lang/clippy.rs` | `rust_lang/clippy_step.rs` |
| `node/typecheck.rs` | `node/typecheck_step.rs` |
| `python/mypy.rs` | `python/mypy_step.rs` |
| `go/vet.rs` | `go/vet_step.rs` |

Base steps extracted from `base/mod.rs` into individual files:

| File | Struct |
|------|--------|
| `base/git_pull_step.rs` | `GitPullStep` |
| `base/build_step.rs` | `BuildStep` |
| `base/test_step.rs` | `TestStep` |
| `base/lint_step.rs` | `LintStep` |
| `base/fmt_step.rs` | `FmtStep` |

### 5. Directory rename

```
src/ci/builder/  →  src/ci/pipeline_builder/
```

All imports across the codebase updated accordingly.

### 6. Final directory structure

```
src/ci/pipeline_builder/
  mod.rs                    StepDef trait, StepConfig struct, PipelineStrategy trait,
                            generate_pipeline(), write_step_report(), strategy_for_pipeline()
  test_parser.rs            TestSummary (used by TestStep)
  base/
    mod.rs                  re-exports all base steps + helper functions (count_pattern)
    git_pull_step.rs        GitPullStep
    build_step.rs           BuildStep
    test_step.rs            TestStep
    lint_step.rs            LintStep
    fmt_step.rs             FmtStep
  maven/
    mod.rs                  MavenStrategy
    checkstyle_step.rs      CheckstyleStep
    package_step.rs         PackageStep
    pmd_step.rs             PmdStep
    spotbugs_step.rs        SpotbugsStep
  gradle/
    mod.rs                  GradleStrategy
    checkstyle_step.rs      CheckstyleStep
    pmd_step.rs             PmdStep
    spotbugs_step.rs        SpotbugsStep
  rust_lang/
    mod.rs                  RustStrategy
    clippy_step.rs          ClippyStep
  node/
    mod.rs                  NodeStrategy
    typecheck_step.rs       TypecheckStep
  python/
    mod.rs                  PythonStrategy
    mypy_step.rs            MypyStep
  go/
    mod.rs                  GoStrategy
    vet_step.rs             VetStep
```

### 7. CLI integration change

In `cmd_run`, instead of looking up strategy to call `output_report_str`:

```rust
// Before: strategy dispatches by step name string
let summary = strategy.output_report_str(step_name, success, &stdout, &stderr);
let log_path = write_step_report(&misc_dir, step_name, &stdout, &stderr);

// After: each step handles its own report
let summary = step.output_report_str(result.success, &stdout, &stderr);
let log_path = step.output_report_path(&misc_dir, &stdout, &stderr);
```

The `generate_pipeline` function returns both the `Pipeline` (for YAML/executor) and the `Vec<Box<dyn StepDef>>` (for report generation). Or, the step objects are kept alongside execution so each step's report methods are available after execution.

### 8. Extensibility

Adding a new step:
1. Create `xxx_step.rs` in the language directory
2. Define struct + `impl StepDef`
3. Add `pub mod xxx_step;` to the language's `mod.rs`
4. Use it in the strategy's `steps()` method

No changes needed to base code, CLI, or output modules.

### 9. Test strategy

- Each `*_step.rs` has unit tests for `config()` and `output_report_str()`
- Strategy tests verify step ordering and dependencies
- Integration tests unchanged (they use pipeline.yml directly)
- Existing test count should remain the same or increase
