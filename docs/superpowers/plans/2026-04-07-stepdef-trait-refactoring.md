# StepDef Trait Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor StepDef from a data struct to a trait, extract each step into its own `*_step.rs` file, and rename `builder/` to `pipeline_builder/`.

**Architecture:** StepDef becomes a trait with `config()`, `output_report_str()`, and `output_report_path()`. Each step is a struct implementing the trait. PipelineStrategy returns `Vec<Box<dyn StepDef>>`. The `builder/` directory is renamed to `pipeline_builder/`.

**Tech Stack:** Rust, traits, Box<dyn>, chrono

---

## File Structure

### New files (create)
- `src/ci/pipeline_builder/mod.rs` — StepDef trait, StepConfig struct, PipelineStrategy trait, generate_pipeline(), write_step_report()
- `src/ci/pipeline_builder/test_parser.rs` — TestSummary (copy from old)
- `src/ci/pipeline_builder/base/mod.rs` — re-exports, count_pattern helper
- `src/ci/pipeline_builder/base/git_pull_step.rs` — GitPullStep
- `src/ci/pipeline_builder/base/build_step.rs` — BuildStep
- `src/ci/pipeline_builder/base/test_step.rs` — TestStep
- `src/ci/pipeline_builder/base/lint_step.rs` — LintStep
- `src/ci/pipeline_builder/base/fmt_step.rs` — FmtStep
- `src/ci/pipeline_builder/maven/mod.rs` — MavenStrategy
- `src/ci/pipeline_builder/maven/checkstyle_step.rs` — CheckstyleStep
- `src/ci/pipeline_builder/maven/package_step.rs` — PackageStep
- `src/ci/pipeline_builder/maven/pmd_step.rs` — PmdStep
- `src/ci/pipeline_builder/maven/spotbugs_step.rs` — SpotbugsStep
- `src/ci/pipeline_builder/gradle/mod.rs` — GradleStrategy
- `src/ci/pipeline_builder/gradle/checkstyle_step.rs` — CheckstyleStep
- `src/ci/pipeline_builder/gradle/pmd_step.rs` — PmdStep
- `src/ci/pipeline_builder/gradle/spotbugs_step.rs` — SpotbugsStep
- `src/ci/pipeline_builder/rust_lang/mod.rs` — RustStrategy
- `src/ci/pipeline_builder/rust_lang/clippy_step.rs` — ClippyStep
- `src/ci/pipeline_builder/node/mod.rs` — NodeStrategy
- `src/ci/pipeline_builder/node/typecheck_step.rs` — TypecheckStep
- `src/ci/pipeline_builder/python/mod.rs` — PythonStrategy
- `src/ci/pipeline_builder/python/mypy_step.rs` — MypyStep
- `src/ci/pipeline_builder/go/mod.rs` — GoStrategy
- `src/ci/pipeline_builder/go/vet_step.rs` — VetStep

### Delete (old directory)
- `src/ci/builder/` — entire directory removed after new one is verified

### Modify
- `src/ci/mod.rs` — change `pub mod builder` to `pub mod pipeline_builder`
- `src/cli/mod.rs` — update all `crate::ci::builder` imports to `crate::ci::pipeline_builder`
- `src/ci/output/tty.rs` — update import
- `src/ci/output/plain.rs` — update import
- `src/run_state/mod.rs` — update import
- `src/ci/detector/base/mod.rs` — update import
- `tests/json_output_test.rs` — if any imports reference builder

---

### Task 1: Create pipeline_builder/mod.rs with StepDef trait and StepConfig

**Files:**
- Create: `src/ci/pipeline_builder/mod.rs`

- [ ] **Step 1: Create the new module file with StepDef trait, StepConfig, PipelineStrategy**

```rust
// src/ci/pipeline_builder/mod.rs
pub mod base;
pub mod maven;
pub mod gradle;
pub mod rust_lang;
pub mod node;
pub mod python;
pub mod go;
pub mod test_parser;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ci::detector::{ProjectInfo, ProjectType};
use crate::ci::parser::{OnFailure, Pipeline, Step};

/// Pure data describing a pipeline step's configuration.
#[derive(Debug, Clone)]
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

impl Default for StepConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            commands: vec![],
            depends_on: vec![],
            workdir: "/workspace".into(),
            on_failure: None,
            allow_failure: false,
            volumes: vec![],
        }
    }
}

impl From<StepConfig> for Step {
    fn from(sc: StepConfig) -> Self {
        Step {
            name: sc.name,
            image: sc.image,
            commands: sc.commands,
            depends_on: sc.depends_on,
            workdir: sc.workdir,
            on_failure: sc.on_failure,
            allow_failure: sc.allow_failure,
            volumes: sc.volumes,
            env: HashMap::new(),
            condition: None,
        }
    }
}

/// Every pipeline step must implement this trait.
pub trait StepDef: Send + Sync {
    /// Return the step's static configuration.
    fn config(&self) -> StepConfig;

    /// Parse stdout/stderr into a one-line human-readable summary.
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String;

    /// Write stdout/stderr to a timestamped log file, return the path.
    fn output_report_path(&self, misc_dir: &Path, stdout: &str, stderr: &str) -> PathBuf {
        write_step_report(misc_dir, &self.config().name, stdout, stderr)
    }
}

/// All language strategies implement this trait.
pub trait PipelineStrategy {
    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>>;
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
}

fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        ProjectType::Maven => Box::new(maven::MavenStrategy),
        ProjectType::Gradle => Box::new(gradle::GradleStrategy),
        ProjectType::Rust => Box::new(rust_lang::RustStrategy),
        ProjectType::Node => Box::new(node::NodeStrategy),
        ProjectType::Python => Box::new(python::PythonStrategy),
        ProjectType::Go => Box::new(go::GoStrategy),
    }
}

/// Get strategy by pipeline name prefix (for report generation after execution).
pub fn strategy_for_pipeline(pipeline: &Pipeline) -> Option<Box<dyn PipelineStrategy>> {
    let name = &pipeline.name;
    if name.starts_with("maven") {
        Some(Box::new(maven::MavenStrategy))
    } else if name.starts_with("gradle") {
        Some(Box::new(gradle::GradleStrategy))
    } else if name.starts_with("rust") {
        Some(Box::new(rust_lang::RustStrategy))
    } else if name.starts_with("node") {
        Some(Box::new(node::NodeStrategy))
    } else if name.starts_with("python") {
        Some(Box::new(python::PythonStrategy))
    } else if name.starts_with("go") {
        Some(Box::new(go::GoStrategy))
    } else {
        None
    }
}

/// Generate a Pipeline from ProjectInfo. Returns Pipeline (for YAML/executor) and
/// Vec<Box<dyn StepDef>> (for report generation).
/// A fixed git-pull step is always prepended; all root steps depend on it.
pub fn generate_pipeline(info: &ProjectInfo) -> (Pipeline, Vec<Box<dyn StepDef>>) {
    let strategy = strategy_for(&info.project_type);
    let mut step_defs = strategy.steps(info);
    let name = strategy.pipeline_name(info);

    // Prepend git-pull and wire root steps to depend on it
    let git_pull = Box::new(base::git_pull_step::GitPullStep);
    let git_pull_name = git_pull.config().name.clone();

    for sd in &mut step_defs {
        let mut config = sd.config();
        if config.depends_on.is_empty() {
            // We can't mutate through Box<dyn StepDef>, so we handle this
            // at the Pipeline level below
        }
        drop(config);
    }

    // Build all configs, fix depends_on, assemble Pipeline
    let mut all_steps: Vec<Box<dyn StepDef>> = vec![git_pull];
    all_steps.extend(step_defs);

    let steps: Vec<Step> = all_steps.iter().map(|sd| {
        let mut config = sd.config();
        if config.name != git_pull_name && config.depends_on.is_empty() {
            config.depends_on.push(git_pull_name.clone());
        }
        config.into()
    }).collect();

    let pipeline = Pipeline {
        name,
        env: HashMap::new(),
        steps,
    };

    (pipeline, all_steps)
}

/// Write step stdout+stderr to pipelight-misc/{step_name}-{timestamp}.log.
/// Always writes (success or failure). Returns the written file path.
pub fn write_step_report(misc_dir: &Path, step_name: &str, stdout: &str, stderr: &str) -> PathBuf {
    let timestamp = chrono::Local::now().format("%Y%m%dT%H%M%S");
    let filename = format!("{}-{}.log", step_name, timestamp);
    let log_path = misc_dir.join(&filename);

    let mut content = String::new();
    if !stdout.is_empty() {
        content.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(stderr);
    }

    if let Err(e) = std::fs::write(&log_path, &content) {
        tracing::warn!("Failed to write step report to {}: {}", log_path.display(), e);
    }

    log_path
}

/// Count lines matching any of the given patterns.
pub fn count_pattern(output: &str, patterns: &[&str]) -> usize {
    output.lines()
        .filter(|line| patterns.iter().any(|p| line.contains(p)))
        .count()
}
```

- [ ] **Step 2: Copy test_parser.rs unchanged**

```bash
cp src/ci/builder/test_parser.rs src/ci/pipeline_builder/test_parser.rs
```

Then update its internal references: the file has no `crate::ci::builder` imports, so it needs no changes.

- [ ] **Step 3: Verify the module compiles (will fail until submodules exist — that's expected)**

---

### Task 2: Create base step files (5 files)

**Files:**
- Create: `src/ci/pipeline_builder/base/mod.rs`
- Create: `src/ci/pipeline_builder/base/git_pull_step.rs`
- Create: `src/ci/pipeline_builder/base/build_step.rs`
- Create: `src/ci/pipeline_builder/base/test_step.rs`
- Create: `src/ci/pipeline_builder/base/lint_step.rs`
- Create: `src/ci/pipeline_builder/base/fmt_step.rs`

- [ ] **Step 1: Create base/mod.rs with re-exports**

```rust
// src/ci/pipeline_builder/base/mod.rs
pub mod git_pull_step;
pub mod build_step;
pub mod test_step;
pub mod lint_step;
pub mod fmt_step;

// Re-export step types for convenience
pub use git_pull_step::GitPullStep;
pub use build_step::BuildStep;
pub use test_step::TestStep;
pub use lint_step::LintStep;
pub use fmt_step::FmtStep;
```

- [ ] **Step 2: Create git_pull_step.rs**

```rust
// src/ci/pipeline_builder/base/git_pull_step.rs
use std::path::{Path, PathBuf};
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct GitPullStep;

impl StepDef for GitPullStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "git-pull".into(),
            image: "alpine/git:latest".into(),
            commands: vec![
                "if [ ! -d .git ]; then echo 'Not a git repository, skipping'; exit 0; fi".into(),
                "if ! git remote | grep -q .; then echo 'No remote configured, skipping'; exit 0; fi".into(),
                "echo \"Pulling from $(git remote get-url origin 2>/dev/null || git remote get-url $(git remote | head -1))...\"".into(),
                "STASHED=false; if ! git diff --quiet || ! git diff --cached --quiet; then echo 'Stashing local changes...'; git stash && STASHED=true; fi".into(),
                "git pull --rebase || { if $STASHED; then git stash pop; fi; echo 'ERROR: git pull --rebase failed — possible merge conflict'; exit 1; }".into(),
                "if $STASHED; then echo 'Restoring stashed changes...'; git stash pop || { echo 'ERROR: stash pop conflict — run git stash pop manually'; exit 1; }; fi".into(),
            ],
            volumes: vec![
                "~/.ssh:/root/.ssh:ro".into(),
                "~/.gitconfig:/root/.gitconfig:ro".into(),
            ],
            on_failure: Some(OnFailure {
                strategy: Strategy::Abort,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, _success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("Already up to date") || output.contains("Already up-to-date") {
            "Already up to date".into()
        } else if output.contains("skipping") || output.contains("Skipping") {
            output.lines()
                .find(|l| l.contains("skipping"))
                .unwrap_or("Skipped")
                .trim().into()
        } else if output.contains("files changed") || output.contains("file changed") {
            output.lines()
                .find(|l| l.contains("files changed") || l.contains("file changed"))
                .unwrap_or("Pulled latest changes")
                .trim().into()
        } else if output.contains("Pulling") {
            "Pulled latest changes".into()
        } else {
            "OK".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_pull_step_config() {
        let step = GitPullStep;
        let config = step.config();
        assert_eq!(config.name, "git-pull");
        assert_eq!(config.image, "alpine/git:latest");
        assert!(config.depends_on.is_empty());
        assert!(config.volumes.iter().any(|v| v.contains(".ssh")));
    }

    #[test]
    fn test_git_pull_report_up_to_date() {
        let step = GitPullStep;
        assert_eq!(step.output_report_str(true, "Already up to date\n", ""), "Already up to date");
    }

    #[test]
    fn test_git_pull_report_skipped() {
        let step = GitPullStep;
        assert_eq!(step.output_report_str(true, "Not a git repository, skipping\n", ""), "Not a git repository, skipping");
    }
}
```

- [ ] **Step 3: Create build_step.rs**

```rust
// src/ci/pipeline_builder/base/build_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct BuildStep {
    image: String,
    build_cmd: Vec<String>,
    source_paths: Vec<String>,
    config_files: Vec<String>,
}

impl BuildStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            build_cmd: info.build_cmd.clone(),
            source_paths: info.source_paths.clone(),
            config_files: info.config_files.clone(),
        }
    }
}

impl StepDef for BuildStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "build".into(),
            image: self.image.clone(),
            commands: self.build_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 3,
                context_paths: [&self.source_paths[..], &self.config_files[..]].concat(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warning_count = count_pattern(&output, &["warning:", "WARNING", "[WARNING]"]);
        if success {
            if warning_count > 0 {
                format!("Build succeeded ({} warnings)", warning_count)
            } else {
                "Build succeeded".into()
            }
        } else {
            let error_count = count_pattern(&output, &["error:", "ERROR", "[ERROR]"]);
            if error_count > 0 {
                format!("Build failed ({} errors)", error_count)
            } else {
                "Build failed".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_build_step_config() {
        let step = BuildStep::new(&make_info());
        let config = step.config();
        assert_eq!(config.name, "build");
        assert_eq!(config.image, "rust:1.78-slim");
        assert_eq!(config.commands, vec!["cargo build"]);
        assert!(config.depends_on.is_empty());
    }

    #[test]
    fn test_build_report_success() {
        let step = BuildStep::new(&make_info());
        assert_eq!(step.output_report_str(true, "compiled OK", ""), "Build succeeded");
    }

    #[test]
    fn test_build_report_warnings() {
        let step = BuildStep::new(&make_info());
        assert_eq!(
            step.output_report_str(true, "warning: unused var\nwarning: dead code\n", ""),
            "Build succeeded (2 warnings)"
        );
    }

    #[test]
    fn test_build_report_failure() {
        let step = BuildStep::new(&make_info());
        assert_eq!(
            step.output_report_str(false, "", "error: cannot find\nerror: aborting\n"),
            "Build failed (2 errors)"
        );
    }
}
```

- [ ] **Step 4: Create test_step.rs**

TestStep is special: each language has its own test output parser. The base TestStep uses a generic report; language strategies can provide a custom TestStep subtype or wrap it. The simplest approach: TestStep accepts an optional parser function.

```rust
// src/ci/pipeline_builder/base/test_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct TestStep {
    image: String,
    test_cmd: Vec<String>,
    /// Optional: language-specific test output parser (returns summary string).
    /// If None, uses generic "Tests passed"/"Tests failed".
    test_parser: Option<fn(&str) -> Option<String>>,
}

impl TestStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            test_cmd: info.test_cmd.clone(),
            test_parser: None,
        }
    }

    pub fn with_parser(mut self, parser: fn(&str) -> Option<String>) -> Self {
        self.test_parser = Some(parser);
        self
    }
}

impl StepDef for TestStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "test".into(),
            image: self.image.clone(),
            commands: self.test_cmd.clone(),
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::Notify,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if let Some(parser) = self.test_parser {
            if let Some(summary) = parser(&output) {
                return summary;
            }
        }
        if success { "Tests passed".into() } else { "Tests failed".into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: None,
            framework: None,
            image: "rust:latest".into(),
            build_cmd: vec![],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec![],
            config_files: vec![],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_test_step_config() {
        let step = TestStep::new(&make_info());
        let config = step.config();
        assert_eq!(config.name, "test");
        assert_eq!(config.depends_on, vec!["build"]);
    }

    #[test]
    fn test_generic_report() {
        let step = TestStep::new(&make_info());
        assert_eq!(step.output_report_str(true, "", ""), "Tests passed");
        assert_eq!(step.output_report_str(false, "", ""), "Tests failed");
    }

    #[test]
    fn test_custom_parser() {
        fn my_parser(output: &str) -> Option<String> {
            if output.contains("5 passed") { Some("5 passed, 0 failed".into()) } else { None }
        }
        let step = TestStep::new(&make_info()).with_parser(my_parser);
        assert_eq!(step.output_report_str(true, "5 passed", ""), "5 passed, 0 failed");
    }
}
```

- [ ] **Step 5: Create lint_step.rs**

```rust
// src/ci/pipeline_builder/base/lint_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct LintStep {
    image: String,
    lint_cmd: Vec<String>,
    source_paths: Vec<String>,
}

impl LintStep {
    pub fn new(info: &ProjectInfo) -> Option<Self> {
        info.lint_cmd.as_ref().map(|cmd| Self {
            image: info.image.clone(),
            lint_cmd: cmd.clone(),
            source_paths: info.source_paths.clone(),
        })
    }
}

impl StepDef for LintStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "lint".into(),
            image: self.image.clone(),
            commands: self.lint_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let issues = count_pattern(&output, &["warning:", "WARNING", "[WARN]", "violation", "Violation"]);
        if success {
            if issues > 0 { format!("lint: passed ({} warnings)", issues) }
            else { "lint: no issues found".into() }
        } else {
            if issues > 0 { format!("lint: {} issues found", issues) }
            else { "lint: failed".into() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    #[test]
    fn test_lint_step_none_when_no_cmd() {
        let info = ProjectInfo {
            project_type: ProjectType::Go,
            language_version: None, framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec![], test_cmd: vec![],
            lint_cmd: None, fmt_cmd: None,
            source_paths: vec![], config_files: vec![],
            warnings: vec![], quality_plugins: vec![], subdir: None,
        };
        assert!(LintStep::new(&info).is_none());
    }

    #[test]
    fn test_lint_report_clean() {
        let info = ProjectInfo {
            project_type: ProjectType::Go,
            language_version: None, framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec![], test_cmd: vec![],
            lint_cmd: Some(vec!["golangci-lint run".into()]), fmt_cmd: None,
            source_paths: vec![".".into()], config_files: vec![],
            warnings: vec![], quality_plugins: vec![], subdir: None,
        };
        let step = LintStep::new(&info).unwrap();
        assert_eq!(step.output_report_str(true, "", ""), "lint: no issues found");
    }
}
```

- [ ] **Step 6: Create fmt_step.rs**

```rust
// src/ci/pipeline_builder/base/fmt_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct FmtStep {
    image: String,
    fmt_cmd: Vec<String>,
    source_paths: Vec<String>,
}

impl FmtStep {
    pub fn new(info: &ProjectInfo) -> Option<Self> {
        info.fmt_cmd.as_ref().map(|cmd| Self {
            image: info.image.clone(),
            fmt_cmd: cmd.clone(),
            source_paths: info.source_paths.clone(),
        })
    }
}

impl StepDef for FmtStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "fmt-check".into(),
            image: self.image.clone(),
            commands: self.fmt_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 1,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        if success {
            "fmt-check: passed".into()
        } else {
            let output = format!("{}{}", stdout, stderr);
            let errors = count_pattern(&output, &["error:", "Error"]);
            if errors > 0 { format!("fmt-check: {} errors", errors) }
            else { "fmt-check: failed".into() }
        }
    }
}
```

- [ ] **Step 7: Verify base module compiles**

Run: `cargo check 2>&1 | head -5`
Expected: may still fail until language strategy modules exist.

- [ ] **Step 8: Commit**

```bash
git add src/ci/pipeline_builder/mod.rs src/ci/pipeline_builder/test_parser.rs src/ci/pipeline_builder/base/
git commit -m "feat: add pipeline_builder module with StepDef trait and base steps"
```

---

### Task 3: Create Maven strategy + step files

**Files:**
- Create: `src/ci/pipeline_builder/maven/mod.rs`
- Create: `src/ci/pipeline_builder/maven/checkstyle_step.rs`
- Create: `src/ci/pipeline_builder/maven/package_step.rs`
- Create: `src/ci/pipeline_builder/maven/pmd_step.rs`
- Create: `src/ci/pipeline_builder/maven/spotbugs_step.rs`

- [ ] **Step 1: Create checkstyle_step.rs**

```rust
// src/ci/pipeline_builder/maven/checkstyle_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

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
    fn config(&self) -> StepConfig {
        let cmd = match &self.subdir {
            Some(subdir) => format!("cd {} && mvn checkstyle:check", subdir),
            None => "mvn checkstyle:check".into(),
        };
        StepConfig {
            name: "checkstyle".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.config_files.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let violations = count_pattern(&output, &["violation", "Violation", "[WARN]", "WARNING"]);
        if success {
            if violations > 0 { format!("checkstyle: passed ({} warnings)", violations) }
            else { "checkstyle: no issues found".into() }
        } else {
            if violations > 0 { format!("checkstyle: {} issues found", violations) }
            else { "checkstyle: failed".into() }
        }
    }
}
```

- [ ] **Step 2: Create package_step.rs**

```rust
// src/ci/pipeline_builder/maven/package_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct PackageStep {
    image: String,
    subdir: Option<String>,
}

impl PackageStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self { image: info.image.clone(), subdir: info.subdir.clone() }
    }
}

impl StepDef for PackageStep {
    fn config(&self) -> StepConfig {
        let cmd = match &self.subdir {
            Some(subdir) => format!("cd {} && mvn package -DskipTests", subdir),
            None => "mvn package -DskipTests".into(),
        };
        StepConfig {
            name: "package".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::Abort,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, _stdout: &str, _stderr: &str) -> String {
        if success { "Package created".into() } else { "Package failed".into() }
    }
}
```

- [ ] **Step 3: Create pmd_step.rs**

```rust
// src/ci/pipeline_builder/maven/pmd_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct PmdStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl PmdStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for PmdStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
             mvn pmd:pmd -Dpmd.rulesetfiles=/workspace/pipelight-misc/pmd-ruleset.xml \
             -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; \
             else mvn pmd:pmd \
             -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; fi",
            cd_prefix
        );
        StepConfig {
            name: "pmd".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let violations = count_pattern(&output, &["violation", "Violation"]);
        if success { "pmd: no violations".into() }
        else if violations > 0 { format!("pmd: {} violations", violations) }
        else { "pmd: failed".into() }
    }
}
```

- [ ] **Step 4: Create spotbugs_step.rs**

```rust
// src/ci/pipeline_builder/maven/spotbugs_step.rs
use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::pipeline_builder::{StepConfig, StepDef, count_pattern};

pub struct SpotbugsStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl SpotbugsStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for SpotbugsStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{}if [ -f /workspace/pipelight-misc/spotbugs-exclude.xml ]; then \
             mvn spotbugs:spotbugs -Dspotbugs.excludeFilterFile=/workspace/pipelight-misc/spotbugs-exclude.xml \
             -Dspotbugs.xmlOutputDirectory=/workspace/pipelight-misc/spotbugs-report; \
             else mvn spotbugs:spotbugs \
             -Dspotbugs.xmlOutputDirectory=/workspace/pipelight-misc/spotbugs-report; fi",
            cd_prefix
        );
        StepConfig {
            name: "spotbugs".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: self.source_paths.clone(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let bugs = count_pattern(&output, &["Bug", "bug"]);
        if success { "spotbugs: no bugs found".into() }
        else if bugs > 0 { format!("spotbugs: {} bugs found", bugs) }
        else { "spotbugs: failed".into() }
    }
}
```

- [ ] **Step 5: Create maven/mod.rs with MavenStrategy**

```rust
// src/ci/pipeline_builder/maven/mod.rs
pub mod checkstyle_step;
pub mod package_step;
pub mod pmd_step;
pub mod spotbugs_step;

use regex::Regex;
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{PipelineStrategy, StepDef};
use crate::ci::pipeline_builder::base::{BuildStep, TestStep};

pub struct MavenStrategy;

fn parse_maven_test(output: &str) -> Option<String> {
    let re = Regex::new(r"Tests run: (\d+), Failures: (\d+), Errors: (\d+), Skipped: (\d+)")
        .unwrap();
    let mut total_run: u32 = 0;
    let mut total_failures: u32 = 0;
    let mut total_errors: u32 = 0;
    let mut total_skipped: u32 = 0;
    let mut found = false;
    for cap in re.captures_iter(output) {
        found = true;
        total_run += cap[1].parse::<u32>().unwrap_or(0);
        total_failures += cap[2].parse::<u32>().unwrap_or(0);
        total_errors += cap[3].parse::<u32>().unwrap_or(0);
        total_skipped += cap[4].parse::<u32>().unwrap_or(0);
    }
    if !found { return None; }
    let passed = total_run.saturating_sub(total_failures + total_errors + total_skipped);
    let failed = total_failures + total_errors;
    Some(format!("{} passed, {} failed, {} skipped", passed, failed, total_skipped))
}

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>> {
        let mut steps: Vec<Box<dyn StepDef>> = vec![
            Box::new(BuildStep::new(info)),
        ];
        let mut quality_step_names: Vec<String> = vec![];

        if info.lint_cmd.is_some() {
            steps.push(Box::new(checkstyle_step::CheckstyleStep::new(info)));
            quality_step_names.push("checkstyle".into());
        }
        steps.push(Box::new(spotbugs_step::SpotbugsStep::new(info)));
        quality_step_names.push("spotbugs".into());
        steps.push(Box::new(pmd_step::PmdStep::new(info)));
        quality_step_names.push("pmd".into());

        // Test step with Maven parser, depends on quality steps
        let mut test_step = TestStep::new(info).with_parser(parse_maven_test);
        // Override depends_on: quality steps instead of just "build"
        // We need a wrapper for this — use a newtype
        steps.push(Box::new(MavenTestStep {
            inner: test_step,
            depends_on: quality_step_names,
        }));

        steps.push(Box::new(package_step::PackageStep::new(info)));

        // Add Maven cache volumes to all steps
        // Note: volumes are set in StepConfig, which is immutable from trait.
        // We handle this by having each Maven step include cache volumes,
        // or we add volumes in generate_pipeline. For simplicity, Maven-specific
        // steps can include the cache volume in their config().
        // Base steps (BuildStep, TestStep) don't have Maven cache — we wrap them.

        steps
    }
}

/// Maven test step: wraps TestStep with custom depends_on and Maven cache volume.
struct MavenTestStep {
    inner: TestStep,
    depends_on: Vec<String>,
}

impl StepDef for MavenTestStep {
    fn config(&self) -> crate::ci::pipeline_builder::StepConfig {
        let mut config = self.inner.config();
        config.depends_on = self.depends_on.clone();
        config.volumes = vec!["~/.m2:/root/.m2".to_string()];
        config
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        self.inner.output_report_str(success, stdout, stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_maven_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: Some(vec!["mvn checkstyle:check".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_maven_steps_with_checkstyle() {
        let info = make_maven_info();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(names, vec!["build", "checkstyle", "spotbugs", "pmd", "test", "package"]);
    }

    #[test]
    fn test_maven_test_report() {
        let output = "Tests run: 42, Failures: 0, Errors: 0, Skipped: 2";
        assert_eq!(parse_maven_test(output).unwrap(), "40 passed, 0 failed, 2 skipped");
    }

    #[test]
    fn test_maven_pipeline_name() {
        let info = make_maven_info();
        assert_eq!(MavenStrategy.pipeline_name(&info), "maven-java-ci");
    }
}
```

- [ ] **Step 6: Commit**

```bash
git add src/ci/pipeline_builder/maven/
git commit -m "feat: add Maven pipeline strategy with StepDef trait steps"
```

---

### Task 4: Create remaining language strategies (Gradle, Rust, Node, Python, Go)

Each follows the same pattern as Maven. Create step files + mod.rs for each.

**Files:**
- Create: `src/ci/pipeline_builder/gradle/mod.rs`, `gradle/checkstyle_step.rs`, `gradle/pmd_step.rs`, `gradle/spotbugs_step.rs`
- Create: `src/ci/pipeline_builder/rust_lang/mod.rs`, `rust_lang/clippy_step.rs`
- Create: `src/ci/pipeline_builder/node/mod.rs`, `node/typecheck_step.rs`
- Create: `src/ci/pipeline_builder/python/mod.rs`, `python/mypy_step.rs`
- Create: `src/ci/pipeline_builder/go/mod.rs`, `go/vet_step.rs`

- [ ] **Step 1: Create all Gradle step files and mod.rs**

Gradle steps are nearly identical to Maven equivalents. CheckstyleStep uses `./gradlew check -x test`. PmdStep/SpotbugsStep use `./gradlew pmdMain`/`./gradlew spotbugsMain` with report copy. GradleStrategy has its own test parser and adds `~/.gradle:/root/.gradle` cache volume.

- [ ] **Step 2: Create rust_lang/clippy_step.rs and mod.rs**

ClippyStep: commands `cargo clippy -- -D warnings`, report counts clippy warnings. RustStrategy uses Rust test parser (`test result: ok. N passed; N failed; N ignored`).

- [ ] **Step 3: Create node/typecheck_step.rs and mod.rs**

TypecheckStep: commands `npx tsc --noEmit`, report counts errors. NodeStrategy uses Jest/Mocha test parser.

- [ ] **Step 4: Create python/mypy_step.rs and mod.rs**

MypyStep: commands `pip install mypy && mypy .`, report counts errors. PythonStrategy uses pytest parser.

- [ ] **Step 5: Create go/vet_step.rs and mod.rs**

VetStep: commands `go vet ./...`, report counts vet findings. GoStrategy uses Go test parser (`ok`/`FAIL` lines).

- [ ] **Step 6: Verify everything compiles**

Run: `cargo check 2>&1 | grep "^error" | head -10`
Expected: No errors (may have warnings about unused old builder module).

- [ ] **Step 7: Commit**

```bash
git add src/ci/pipeline_builder/gradle/ src/ci/pipeline_builder/rust_lang/ src/ci/pipeline_builder/node/ src/ci/pipeline_builder/python/ src/ci/pipeline_builder/go/
git commit -m "feat: add Gradle/Rust/Node/Python/Go strategies with StepDef trait steps"
```

---

### Task 5: Switch module declaration and update all imports

**Files:**
- Modify: `src/ci/mod.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/ci/output/tty.rs`
- Modify: `src/ci/output/plain.rs`
- Modify: `src/run_state/mod.rs`
- Modify: `src/ci/detector/base/mod.rs`

- [ ] **Step 1: Update src/ci/mod.rs**

```rust
// Change:
//   pub mod builder;
// To:
pub mod pipeline_builder;
```

- [ ] **Step 2: Global find-and-replace all imports**

Replace `crate::ci::builder` with `crate::ci::pipeline_builder` in all files:
- `src/cli/mod.rs`
- `src/ci/output/tty.rs`
- `src/ci/output/plain.rs`
- `src/run_state/mod.rs`
- `src/ci/detector/base/mod.rs`

Also update specific type references:
- `StepDef` (the old struct) no longer exists — in executor/CLI, use `StepConfig` where a data struct is needed
- `strategy_for_pipeline` now returns a `PipelineStrategy` (same function name, same usage)

- [ ] **Step 3: Update CLI cmd_run to use new generate_pipeline signature**

`generate_pipeline` now returns `(Pipeline, Vec<Box<dyn StepDef>>)`. Update `cmd_run`:

```rust
// In cmd_init:
let (info, pipeline) = detector::detect_and_generate(&dir)?;
// detect_and_generate still returns (ProjectInfo, Pipeline) — it calls generate_pipeline internally.
// Update detect_and_generate to use new signature, discarding step objects for init.

// In cmd_run, after getting pipeline:
// Use step objects for report generation instead of strategy_for_pipeline lookup
```

- [ ] **Step 4: Update detect_and_generate in detector/base/mod.rs**

The function calls `generate_pipeline(info)` — update to handle new return type `(Pipeline, Vec<Box<dyn StepDef>>)`, returning only the Pipeline.

- [ ] **Step 5: Verify compilation**

Run: `cargo check 2>&1 | grep "^error" | head -20`
Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add src/ci/mod.rs src/cli/mod.rs src/ci/output/ src/run_state/ src/ci/detector/
git commit -m "refactor: switch from builder to pipeline_builder module"
```

---

### Task 6: Delete old builder directory

- [ ] **Step 1: Run all tests to verify new module works**

Run: `cargo test 2>&1 | grep "^test result"`
Expected: All suites pass (same count as before or higher).

- [ ] **Step 2: Delete old directory**

```bash
rm -rf src/ci/builder/
```

- [ ] **Step 3: Run tests again to confirm nothing depended on old module**

Run: `cargo test 2>&1 | grep "^test result"`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: remove old builder directory, pipeline_builder is now canonical"
```

---

### Task 7: Update CLAUDE.md and docs

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/architecture.md`

- [ ] **Step 1: Update CLAUDE.md directory structure**

Change `builder/` references to `pipeline_builder/` and update the module descriptions to reflect the StepDef trait architecture.

- [ ] **Step 2: Update docs/architecture.md**

Update module descriptions: pipeline_builder uses StepDef trait, each step is a `*_step.rs` file implementing the trait.

- [ ] **Step 3: Run full test suite one final time**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 4: Commit and push**

```bash
git add CLAUDE.md docs/
git commit -m "docs: update architecture docs for pipeline_builder refactoring"
git push
```
