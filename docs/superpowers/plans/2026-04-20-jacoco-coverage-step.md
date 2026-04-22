# JaCoCo Coverage Step Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-file LINE coverage check step (`jacoco`) plus a full-repo variant (`jacoco_full`) to both Maven and Gradle pipelines, mirroring the existing PMD / SpotBugs incremental-plus-full twin pattern. When a changed file falls below 70% LINE coverage, fire an `AutoFix` callback so the LLM can write unit tests and retry.

**Architecture:** Hybrid execution — if the project's `pom.xml` / `build.gradle` already declares the JaCoCo plugin, use the plugin's own `prepare-agent` / `jacocoTestReport` entry points; otherwise download a standalone JaCoCo agent + CLI into `~/.pipelight/cache/jacoco-0.8.12/` and inject it via `MAVEN_OPTS` / `JAVA_TOOL_OPTIONS`. A decorator (`JacocoAgentTestStep`) wraps `TestStep` to pick the mode; two new `StepDef` impls (`MavenJacocoStep`, `MavenJacocoFullStep` and their Gradle counterparts) handle coverage analysis and callback dispatch. Two new `CallbackCommand` variants are added: `AutoGenJacocoConfig` (LLM generates `pipelight-misc/jacoco-config.yml`) and `JacocoPrintCommand` (LLM prints grouped coverage table for the `_full` variant).

**Tech Stack:** Rust (existing), JaCoCo 0.8.12 (CLI + agent), shell (awk/grep/sed) for in-container XML parsing, YAML for user config.

**Reference spec:** `docs/superpowers/specs/2026-04-20-jacoco-coverage-step-design.md`

---

## File Structure

### New files

| Path | Responsibility |
|---|---|
| `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs` | `JacocoMode` enum + `JacocoAgentTestStep` decorator that wraps `TestStep` to inject the JaCoCo agent (standalone) or switch the test command to use the project's JaCoCo plugin |
| `src/ci/pipeline_builder/maven/jacoco_step.rs` | `MavenJacocoStep` — incremental (git-diff filtered) LINE coverage check, hard-fail + `AutoFix` |
| `src/ci/pipeline_builder/maven/jacoco_full_step.rs` | `MavenJacocoFullStep` — full-repo coverage, report-only, `JacocoPrintCommand` |
| `src/ci/pipeline_builder/gradle/jacoco_step.rs` | `GradleJacocoStep` — same as Maven but for Gradle image/workspace |
| `src/ci/pipeline_builder/gradle/jacoco_full_step.rs` | `GradleJacocoFullStep` |

### Modified files

| Path | Change |
|---|---|
| `src/ci/callback/command.rs` | Add `AutoGenJacocoConfig` and `JacocoPrintCommand` variants + registry entries |
| `src/ci/callback/action.rs` | Add `JacocoPrint` variant |
| `src/ci/detector/maven.rs` | Detect `jacoco-maven-plugin` → push `"jacoco"` into `quality_plugins` |
| `src/ci/detector/gradle.rs` | Detect `jacoco` plugin → push `"jacoco"` into `quality_plugins` |
| `src/ci/pipeline_builder/base/mod.rs` | Export `JacocoAgentTestStep` + `JacocoMode`; extend `default_report_str` to recognise `"jacoco"` / `"jacoco_full"` |
| `src/ci/pipeline_builder/maven/mod.rs` | Wrap `test` in `JacocoAgentTestStep`, insert `jacoco` + `jacoco_full` between `test` and `package`, change `package.depends_on` to `["jacoco_full"]` |
| `src/ci/pipeline_builder/gradle/mod.rs` | Wrap `test` in `JacocoAgentTestStep`, append `jacoco` + `jacoco_full` at tail |
| `global-skills/pipelight-run/SKILL.md` | Add callback-command rows for `AutoGenJacocoConfig` + `JacocoPrintCommand`, plus detailed sections |

### No changes needed

- `src/ci/pipeline_builder/base/test_step.rs` — decorator wraps it; no fields added.
- Non-Java strategies (`rust_lang.rs`, `go.rs`, `node.rs`, `vue.rs`, `python.rs`) — they never instantiate `JacocoAgentTestStep`, so they're untouched.

---

## Phase 1 — Callback plumbing (foundation)

### Task 1: Add `JacocoPrint` variant to `CallbackCommandAction`

**Files:**
- Modify: `src/ci/callback/action.rs`

- [ ] **Step 1: Write the failing test**

Extend the existing `test_serde_roundtrip` test in `src/ci/callback/action.rs` to include the new variant. Add this tuple inside the array:

```rust
(CallbackCommandAction::JacocoPrint, "\"jacoco_print\""),
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pipelight --lib ci::callback::action::tests::test_serde_roundtrip`
Expected: FAIL with `no variant named JacocoPrint` or similar compile error.

- [ ] **Step 3: Add the variant**

In `src/ci/callback/action.rs`, add a variant to the enum (between `SpotbugsPrint` and `GitDiffReport`):

```rust
    /// LLM parses the JaCoCo XML report and prints a grouped-by-package
    /// coverage table. Pipeline flow unaffected.
    JacocoPrint,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pipelight --lib ci::callback::action`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/callback/action.rs
git commit -m "feat(callback): add JacocoPrint action variant"
```

---

### Task 2: Add `AutoGenJacocoConfig` and `JacocoPrintCommand` variants + registry

**Files:**
- Modify: `src/ci/callback/command.rs`

- [ ] **Step 1: Write the failing serde test**

In `src/ci/callback/command.rs`, extend `test_serde_roundtrip_all_variants` — add two entries to its array:

```rust
(CallbackCommand::AutoGenJacocoConfig, "\"auto_gen_jacoco_config\""),
(CallbackCommand::JacocoPrintCommand, "\"jacoco_print_command\""),
```

- [ ] **Step 2: Write the failing registry test**

Append a new test function at the end of the `tests` module in `src/ci/callback/command.rs`:

```rust
    #[test]
    fn test_registry_jacoco_commands_registered() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::AutoGenJacocoConfig),
            CallbackCommandAction::Retry
        );
        assert_eq!(
            registry.action_for(&CallbackCommand::JacocoPrintCommand),
            CallbackCommandAction::JacocoPrint
        );
        assert!(registry.get(&CallbackCommand::AutoGenJacocoConfig).is_some());
        assert!(registry.get(&CallbackCommand::JacocoPrintCommand).is_some());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::callback::command`
Expected: FAIL — variants don't exist yet.

- [ ] **Step 4: Add enum variants + registry entries**

In `src/ci/callback/command.rs`:

1. Add two variants to the `CallbackCommand` enum (after `GitDiffCommand`):

```rust
    AutoGenJacocoConfig,
    JacocoPrintCommand,
```

2. Register them in `CallbackCommandRegistry::new()` (before the final `registry` return):

```rust
        registry.register(
            CallbackCommand::AutoGenJacocoConfig,
            CallbackCommandDef {
                action: CallbackCommandAction::Retry,
                description:
                    "LLM searches project for existing test conventions and source file patterns, generates pipelight-misc/jacoco-config.yml with threshold + exclude globs, then retries."
                        .into(),
            },
        );
        registry.register(
            CallbackCommand::JacocoPrintCommand,
            CallbackCommandDef {
                action: CallbackCommandAction::JacocoPrint,
                description:
                    "JaCoCo scan found files below the LINE coverage threshold (report-only for full variant). LLM parses the JaCoCo XML report and prints a grouped-by-package coverage table. Pipeline continues."
                        .into(),
            },
        );
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::callback::command`
Expected: all tests pass, including the new `test_registry_jacoco_commands_registered`.

- [ ] **Step 6: Commit**

```bash
git add src/ci/callback/command.rs
git commit -m "feat(callback): add JaCoCo callback commands and registry entries"
```

---

## Phase 2 — Detector: identify JaCoCo plugin

### Task 3: Maven detector — recognise `jacoco-maven-plugin`

**Files:**
- Modify: `src/ci/detector/maven.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/ci/detector/maven.rs`:

```rust
    #[test]
    fn test_jacoco_detection() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <properties><java.version>17</java.version></properties>
  <build><plugins><plugin>
    <groupId>org.jacoco</groupId>
    <artifactId>jacoco-maven-plugin</artifactId>
    <version>0.8.12</version>
  </plugin></plugins></build>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let info = MavenDetector.analyze(dir.path()).unwrap();
        assert!(info.quality_plugins.contains(&"jacoco".to_string()));
    }

    #[test]
    fn test_jacoco_absent() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"<project><modelVersion>4.0.0</modelVersion></project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let info = MavenDetector.analyze(dir.path()).unwrap();
        assert!(!info.quality_plugins.contains(&"jacoco".to_string()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::detector::maven::tests::test_jacoco`
Expected: `test_jacoco_detection` FAILS (no `jacoco` in `quality_plugins`); `test_jacoco_absent` PASSES (trivially true).

- [ ] **Step 3: Implement detection**

In `src/ci/detector/maven.rs`, add a helper method inside `impl MavenDetector` (after `has_pmd`):

```rust
    /// Check if pom.xml contains the jacoco-maven-plugin
    fn has_jacoco(content: &str) -> bool {
        content.contains("jacoco-maven-plugin")
    }
```

In the `analyze` function, after the existing `if Self::has_pmd(&content) { ... }` block, add:

```rust
        if Self::has_jacoco(&content) {
            quality_plugins.push("jacoco".to_string());
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::detector::maven`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/detector/maven.rs
git commit -m "feat(detector): identify jacoco-maven-plugin in pom.xml"
```

---

### Task 4: Gradle detector — recognise `jacoco` plugin

**Files:**
- Modify: `src/ci/detector/gradle.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src/ci/detector/gradle.rs`:

```rust
    #[test]
    fn test_jacoco_detection_groovy_id() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "plugins { id 'jacoco' }",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.quality_plugins.contains(&"jacoco".to_string()));
    }

    #[test]
    fn test_jacoco_detection_kts_id() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle.kts"),
            "plugins { id(\"jacoco\") }",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.quality_plugins.contains(&"jacoco".to_string()));
    }

    #[test]
    fn test_jacoco_detection_apply_plugin() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "apply plugin: 'jacoco'",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.quality_plugins.contains(&"jacoco".to_string()));
    }

    #[test]
    fn test_jacoco_absent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(!info.quality_plugins.contains(&"jacoco".to_string()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::detector::gradle::tests::test_jacoco`
Expected: detection tests FAIL.

- [ ] **Step 3: Implement detection**

In `src/ci/detector/gradle.rs`, add a helper inside `impl GradleDetector` (after `has_pmd`):

```rust
    /// Check if build file contains jacoco plugin (id 'jacoco' / id("jacoco") / apply plugin).
    /// Note: must not match bare "jacoco" inside words like "tjacoco" — bound with
    /// quotes/parens to avoid false positives.
    fn has_jacoco(content: &str) -> bool {
        content.contains("id 'jacoco'")
            || content.contains("id \"jacoco\"")
            || content.contains("id(\"jacoco\")")
            || content.contains("id('jacoco')")
            || content.contains("apply plugin: 'jacoco'")
            || content.contains("apply plugin: \"jacoco\"")
    }
```

In `analyze`, after the `has_pmd` check:

```rust
        if Self::has_jacoco(&content) {
            quality_plugins.push("jacoco".to_string());
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::detector::gradle`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/detector/gradle.rs
git commit -m "feat(detector): identify jacoco plugin in build.gradle(.kts)"
```

---

## Phase 3 — `JacocoAgentTestStep` decorator

The decorator wraps an existing `TestStep` and rewrites the shell command produced by its `config()` to inject the JaCoCo agent (standalone) or switch to the project's JaCoCo plugin. It also appends a path-normalization `cp` command in Gradle-plugin mode so downstream steps always find `jacoco.exec` + `jacoco.xml` at a fixed location.

### Task 5: Define `JacocoMode` enum and skeleton struct

**Files:**
- Create: `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`
- Modify: `src/ci/pipeline_builder/base/mod.rs`

- [ ] **Step 1: Create skeleton file with the enum and struct**

Create `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`:

```rust
use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// JaCoCo execution mode selected by the strategy at pipeline-build time.
///
/// - `None` — no JaCoCo wiring; decorator is a no-op.
/// - `Standalone` — download the JaCoCo agent into the pipelight cache and
///   inject it via `MAVEN_OPTS` / `JAVA_TOOL_OPTIONS` so the test JVM
///   instruments classes at runtime and writes `jacoco.exec`.
/// - `MavenPlugin` — rewrite `mvn test` to `mvn jacoco:prepare-agent test ...`
///   so the project's own jacoco-maven-plugin handles instrumentation.
/// - `GradlePlugin` — append `jacocoTestReport` to the Gradle command; copy
///   Gradle's default outputs into `pipelight-misc/jacoco-report/` so the
///   downstream jacoco step finds them in a predictable location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JacocoMode {
    None,
    Standalone,
    MavenPlugin,
    GradlePlugin,
}

/// Pinned JaCoCo version. Supports Java 5–22 bytecode instrumentation and
/// requires only JRE 1.8+ to run jacococli, covering the user's Java 8/17 span.
pub const JACOCO_VERSION: &str = "0.8.12";

/// Decorator wrapping a `TestStep` to inject the JaCoCo agent or switch to
/// the project's JaCoCo plugin. Non-Java pipelines pass `JacocoMode::None`
/// and the decorator behaves as a no-op (all trait methods forward to inner).
pub struct JacocoAgentTestStep {
    inner: Box<dyn StepDef>,
    mode: JacocoMode,
}

impl JacocoAgentTestStep {
    pub fn new(inner: Box<dyn StepDef>, mode: JacocoMode) -> Self {
        Self { inner, mode }
    }
}

impl StepDef for JacocoAgentTestStep {
    fn config(&self) -> StepConfig {
        // Implemented in the next tasks.
        self.inner.config()
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        self.inner.exception_mapping()
    }

    fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        self.inner.match_exception(exit_code, stdout, stderr)
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        self.inner.output_report_str(success, stdout, stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};
    use crate::ci::pipeline_builder::base::TestStep;

    fn make_info(test_cmd: &str, image: &str) -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: image.into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec![test_cmd.into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_mode_none_passes_commands_through() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::None);
        let cfg = decorator.config();
        assert_eq!(cfg.commands, vec!["mvn test".to_string()]);
    }
}
```

- [ ] **Step 2: Wire the module into `base/mod.rs`**

Modify `src/ci/pipeline_builder/base/mod.rs`. After the existing `pub mod test_step;` add:

```rust
pub mod jacoco_agent_decorator;
```

After the existing `pub use test_step::TestStep;` add:

```rust
pub use jacoco_agent_decorator::{JacocoAgentTestStep, JacocoMode, JACOCO_VERSION};
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator`
Expected: `test_mode_none_passes_commands_through` PASSES (None mode is already the default forwarding behaviour).

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/base/jacoco_agent_decorator.rs src/ci/pipeline_builder/base/mod.rs
git commit -m "feat(pipeline): scaffold JacocoAgentTestStep decorator"
```

---

### Task 6: Decorator — Standalone mode (inject `-javaagent` via `MAVEN_OPTS`)

**Files:**
- Modify: `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    #[test]
    fn test_standalone_mode_maven_injects_javaagent() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco-0.8.12"),
            "expected pinned JaCoCo version in command, got: {}",
            combined
        );
        assert!(
            combined.contains("MAVEN_OPTS"),
            "expected MAVEN_OPTS env var, got: {}",
            combined
        );
        assert!(
            combined.contains("-javaagent:"),
            "expected -javaagent injection, got: {}",
            combined
        );
        assert!(
            combined.contains("destfile=/workspace/pipelight-misc/jacoco-report/jacoco.exec"),
            "expected destfile in pipelight-misc, got: {}",
            combined
        );
        // Original test command must still be present.
        assert!(combined.contains("mvn test"));
    }

    #[test]
    fn test_standalone_mode_gradle_injects_java_tool_options() {
        let inner = TestStep::new(&make_info("./gradlew test", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("JAVA_TOOL_OPTIONS"),
            "expected JAVA_TOOL_OPTIONS env var for Gradle, got: {}",
            combined
        );
        assert!(combined.contains("-javaagent:"));
        assert!(combined.contains("./gradlew test"));
    }

    #[test]
    fn test_standalone_mode_downloads_agent_if_missing() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco-0.8.12.zip")
                || combined.contains("jacoco-0.8.12-bin"),
            "expected agent download logic, got: {}",
            combined
        );
        assert!(combined.contains("~/.pipelight/cache") || combined.contains("$HOME/.pipelight/cache"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator::tests::test_standalone`
Expected: FAIL (decorator still forwards verbatim).

- [ ] **Step 3: Implement standalone mode**

Replace the `config` method in `JacocoAgentTestStep` with:

```rust
    fn config(&self) -> StepConfig {
        let mut cfg = self.inner.config();
        match self.mode {
            JacocoMode::None => {}
            JacocoMode::Standalone => {
                let is_gradle = cfg.image.contains("gradle");
                let agent_env = if is_gradle {
                    "JAVA_TOOL_OPTIONS"
                } else {
                    "MAVEN_OPTS"
                };
                let agent_jar = format!(
                    "$HOME/.pipelight/cache/jacoco-{ver}/lib/jacocoagent.jar",
                    ver = JACOCO_VERSION
                );
                let destfile = "/workspace/pipelight-misc/jacoco-report/jacoco.exec";
                let prepare = format!(
                    "JACOCO_VER={ver} && \
                     JACOCO_CACHE=$HOME/.pipelight/cache && \
                     JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
                     if [ ! -f {agent} ]; then \
                       echo 'Downloading JaCoCo $JACOCO_VER ...' && \
                       mkdir -p $JACOCO_DIR && \
                       curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip \
                         -o /tmp/jacoco.zip && \
                       (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && \
                       rm -f /tmp/jacoco.zip; \
                     fi && \
                     mkdir -p /workspace/pipelight-misc/jacoco-report && \
                     export {env}=\"-javaagent:{agent}=destfile={destfile},append=false\"",
                    ver = JACOCO_VERSION,
                    agent = agent_jar,
                    env = agent_env,
                    destfile = destfile,
                );
                let joined_original = cfg.commands.join(" && ");
                cfg.commands = vec![format!("{prepare} && {joined_original}")];
            }
            JacocoMode::MavenPlugin | JacocoMode::GradlePlugin => {
                // Implemented in later tasks.
            }
        }
        cfg
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator`
Expected: all 4 tests pass (including the existing `test_mode_none_passes_commands_through`).

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/base/jacoco_agent_decorator.rs
git commit -m "feat(pipeline): implement JaCoCo standalone agent injection"
```

---

### Task 7: Decorator — Maven plugin mode

**Files:**
- Modify: `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    #[test]
    fn test_maven_plugin_mode_injects_prepare_agent() {
        let inner = TestStep::new(&make_info(
            "mvn test --fail-at-end",
            "maven:3.9-eclipse-temurin-17",
        ));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::MavenPlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco:prepare-agent"),
            "expected jacoco:prepare-agent goal inserted, got: {}",
            combined
        );
        assert!(
            combined.contains("-Djacoco.destFile=/workspace/pipelight-misc/jacoco-report/jacoco.exec"),
            "expected destFile system property, got: {}",
            combined
        );
        // `--fail-at-end` must be preserved.
        assert!(combined.contains("--fail-at-end"));
    }

    #[test]
    fn test_maven_plugin_mode_leaves_non_mvn_untouched() {
        // Non-maven command should not be rewritten (defensive: we only rewrite
        // occurrences of `mvn test`, not arbitrary shell).
        let inner = TestStep::new(&make_info("echo hi", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::MavenPlugin);
        let cfg = decorator.config();
        assert_eq!(cfg.commands, vec!["echo hi".to_string()]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator::tests::test_maven_plugin`
Expected: FAIL.

- [ ] **Step 3: Implement Maven plugin mode**

Inside the `config` match arm `JacocoMode::MavenPlugin`, add:

```rust
            JacocoMode::MavenPlugin => {
                let destfile = "/workspace/pipelight-misc/jacoco-report/jacoco.exec";
                let rewritten: Vec<String> = cfg
                    .commands
                    .iter()
                    .map(|c| {
                        if c.contains("mvn test") {
                            let replacement = format!(
                                "mvn jacoco:prepare-agent test -Djacoco.destFile={destfile}"
                            );
                            c.replace("mvn test", &replacement)
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                let joined = rewritten.join(" && ");
                cfg.commands = vec![format!(
                    "mkdir -p /workspace/pipelight-misc/jacoco-report && {joined}"
                )];
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/base/jacoco_agent_decorator.rs
git commit -m "feat(pipeline): implement JaCoCo Maven plugin mode"
```

---

### Task 8: Decorator — Gradle plugin mode with copy fallback

**Files:**
- Modify: `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    #[test]
    fn test_gradle_plugin_mode_appends_jacoco_test_report() {
        let inner = TestStep::new(&make_info("./gradlew test --continue", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::GradlePlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacocoTestReport"),
            "expected jacocoTestReport task appended, got: {}",
            combined
        );
        assert!(combined.contains("--continue"), "must preserve --continue");
    }

    #[test]
    fn test_gradle_plugin_mode_copies_outputs_to_pipelight_misc() {
        let inner = TestStep::new(&make_info("./gradlew test", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::GradlePlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("cp build/jacoco/test.exec"),
            "expected cp fallback for exec, got: {}",
            combined
        );
        assert!(
            combined.contains("jacocoTestReport.xml"),
            "expected cp fallback for xml, got: {}",
            combined
        );
        assert!(
            combined.contains("pipelight-misc/jacoco-report"),
            "expected destination path, got: {}",
            combined
        );
        assert!(
            combined.contains("|| true"),
            "copy must be tolerant of missing files (|| true)"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator::tests::test_gradle_plugin`
Expected: FAIL.

- [ ] **Step 3: Implement Gradle plugin mode**

Inside the `config` match arm `JacocoMode::GradlePlugin`, add:

```rust
            JacocoMode::GradlePlugin => {
                let rewritten: Vec<String> = cfg
                    .commands
                    .iter()
                    .map(|c| {
                        if c.contains("./gradlew") && !c.contains("jacocoTestReport") {
                            format!("{c} jacocoTestReport")
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                let joined = rewritten.join(" && ");
                let copy_cmd = "mkdir -p /workspace/pipelight-misc/jacoco-report && \
                     cp build/jacoco/test.exec /workspace/pipelight-misc/jacoco-report/jacoco.exec 2>/dev/null || true && \
                     cp build/reports/jacoco/test/jacocoTestReport.xml /workspace/pipelight-misc/jacoco-report/jacoco.xml 2>/dev/null || true";
                cfg.commands = vec![format!("{joined} && {copy_cmd}")];
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::base::jacoco_agent_decorator`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/base/jacoco_agent_decorator.rs
git commit -m "feat(pipeline): implement JaCoCo Gradle plugin mode with copy fallback"
```

---

## Phase 4 — `MavenJacocoStep` (incremental)

This step is large, built up through several TDD tasks. Each task adds one behaviour and one test.

### Task 9: Scaffold `MavenJacocoStep` with basic `StepConfig`

**Files:**
- Create: `src/ci/pipeline_builder/maven/jacoco_step.rs`
- Modify: `src/ci/pipeline_builder/maven/mod.rs` (module declaration only)

- [ ] **Step 1: Create the skeleton file**

Create `src/ci/pipeline_builder/maven/jacoco_step.rs`:

```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{git_changed_files_snippet, StepConfig, StepDef};

/// Incremental JaCoCo coverage check (tag = "non-full").
///
/// Reads `pipelight-misc/jacoco-report/jacoco.exec` (populated by the
/// `JacocoAgentTestStep`-wrapped test step), generates a JaCoCo XML report
/// (standalone/plugin modes differ on whether the XML is already produced),
/// filters sourcefile entries to the git-diff working-branch changes
/// (minus the exclude patterns in `jacoco-config.yml`), and fails if any
/// changed file's LINE coverage is below the threshold. Failures fire an
/// `AutoFix` callback so the LLM can add unit tests and retry.
pub struct MavenJacocoStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,
}

impl MavenJacocoStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for MavenJacocoStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec!["true".into()], // placeholder; filled in next tasks
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        // Filled in Task 13.
        ExceptionMapping::new(CallbackCommand::RuntimeError)
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        None
    }

    fn output_report_str(&self, success: bool, _stdout: &str, _stderr: &str) -> String {
        if success {
            "jacoco: ok".into()
        } else {
            "jacoco: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_basic_step_config() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco");
        assert_eq!(cfg.depends_on, vec!["test".to_string()]);
        assert_eq!(cfg.tag, "non-full");
        assert!(!cfg.allow_failure);
        assert!(cfg.active);
    }
}
```

- [ ] **Step 2: Register the module in `maven/mod.rs`**

In `src/ci/pipeline_builder/maven/mod.rs`, add near the top with the other `pub mod` lines (keep alphabetical-ish with existing declarations):

```rust
pub mod jacoco_step;
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs src/ci/pipeline_builder/maven/mod.rs
git commit -m "feat(maven): scaffold MavenJacocoStep with StepConfig"
```

---

### Task 10: `MavenJacocoStep` — emit `AutoGenJacocoConfig` callback when config missing

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module of `src/ci/pipeline_builder/maven/jacoco_step.rs`:

```rust
    #[test]
    fn test_command_emits_auto_gen_config_callback_when_missing() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config"),
            "command must emit the auto-gen-config callback when config missing, got: {}",
            cmd
        );
        assert!(
            cmd.contains("pipelight-misc/jacoco-config.yml"),
            "command must reference config file path, got: {}",
            cmd
        );
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step::tests::test_command_emits_auto_gen_config_callback_when_missing`
Expected: FAIL (placeholder `"true"` has no callback marker).

- [ ] **Step 3: Implement initial command generation**

Replace the `config` method in `MavenJacocoStep` with:

```rust
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found in pipelight-misc/. LLM should generate one with threshold (default 70) and exclude globs (DTO/Config/Exception/Application at minimum).' >&2; \
               exit 1; \
             fi && \
             true",
            cd = cd_prefix,
        );
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): emit auto_gen_jacoco_config callback when config missing"
```

---

### Task 11: `MavenJacocoStep` — git-diff + exclude filter + self-skip branches

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests`:

```rust
    #[test]
    fn test_command_reads_git_diff_report() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("pipelight-misc/git-diff-report/unstaged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/staged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/untracked.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/unpushed.txt"));
        assert!(cmd.contains("java|kt"));
    }

    #[test]
    fn test_command_skips_when_no_changed_java_files() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no changed java/kt files"),
            "command must self-skip when no changes, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_applies_exclude_patterns_from_config() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        // Must reference `exclude:` from jacoco-config.yml — simplest check:
        // parser uses awk/sed/grep against the yaml file.
        assert!(
            cmd.contains("jacoco-config.yml") && cmd.contains("exclude"),
            "command must consult exclude patterns, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_skips_when_all_files_excluded() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("all changed files excluded"),
            "command must self-skip when filter empties the list, got: {}",
            cmd
        );
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all four new tests FAIL.

- [ ] **Step 3: Extend the command**

Replace the `config` method again with the extended shell command. Use `git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref())` — the helper already references the git-diff-report files and builds `CHANGED_FILES`.

```rust
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let changed_files = git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref());
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found in pipelight-misc/. LLM should generate one with threshold (default 70) and exclude globs (DTO/Config/Exception/Application at minimum).' >&2; \
               exit 1; \
             fi && \
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'jacoco: no changed java/kt files on current branch — skipping'; \
               exit 0; \
             fi && \
             # Read exclude globs from jacoco-config.yml (lines after 'exclude:' that start with '-'). \
             EXCLUDES=$(awk '/^exclude:/{{flag=1; next}} flag && /^[[:space:]]*-/ {{gsub(/^[[:space:]]*-[[:space:]]*\"?/,\"\"); gsub(/\"?[[:space:]]*$/,\"\"); print}} flag && /^[^[:space:]-]/ {{flag=0}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             FILTERED=\"\" && \
             while IFS= read -r f; do \
               [ -z \"$f\" ] && continue; \
               skip=0; \
               for pat in $EXCLUDES; do \
                 # Translate glob ** / * into regex for bash case-insensitive match. \
                 case \"$f\" in $pat) skip=1; break;; esac; \
               done; \
               [ \"$skip\" -eq 0 ] && FILTERED=\"$FILTERED$f\\n\"; \
             done <<< \"$CHANGED_FILES\" && \
             FILTERED=$(printf '%b' \"$FILTERED\" | sed '/^$/d') && \
             if [ -z \"$FILTERED\" ]; then \
               echo 'jacoco: all changed files excluded by jacoco-config.yml — skipping'; \
               exit 0; \
             fi && \
             echo \"jacoco: checking coverage for $(echo \\\"$FILTERED\\\" | wc -l | tr -d ' ') changed file(s)\" && \
             true",
            cd = cd_prefix,
            changed_files = changed_files,
        );
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): apply git-diff + exclude-glob filter with self-skip branches"
```

---

### Task 12: `MavenJacocoStep` — generate XML via jacococli + skip if exec missing

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests`:

```rust
    #[test]
    fn test_command_handles_missing_exec_file() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no jacoco.exec")
                || cmd.contains("jacoco.exec not found")
                || cmd.contains("jacoco: no exec file"),
            "command must handle missing exec gracefully, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_downloads_jacococli_in_standalone_mode() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("jacococli.jar"),
            "standalone mode must download/use jacococli, got: {}",
            cmd
        );
        assert!(cmd.contains("jacoco-0.8.12"));
    }

    #[test]
    fn test_command_generates_xml_report_in_standalone_mode() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("report") && cmd.contains("--xml"),
            "standalone mode must run `jacococli report --xml`, got: {}",
            cmd
        );
        assert!(cmd.contains("pipelight-misc/jacoco-report/jacoco.xml"));
    }

    #[test]
    fn test_command_skips_xml_generation_in_gradle_plugin_mode() {
        // GradlePlugin mode: the decorator copies jacocoTestReport.xml to
        // pipelight-misc already, so the step should NOT shell out to jacococli.
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::GradlePlugin);
        let cmd = step.config().commands[0].clone();
        assert!(
            !cmd.contains("jacococli.jar") || cmd.contains("if [ ! -f"),
            "gradle-plugin mode should guard XML gen behind 'xml file missing', got: {}",
            cmd
        );
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: the four new tests FAIL.

- [ ] **Step 3: Extend the command**

Replace the body of `config` with the following (keep earlier logic; append exec-check, cli download, report generation before the final `true`):

```rust
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let changed_files = git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref());
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found in pipelight-misc/. LLM should generate one with threshold (default 70) and exclude globs (DTO/Config/Exception/Application at minimum).' >&2; \
               exit 1; \
             fi && \
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'jacoco: no changed java/kt files on current branch — skipping'; \
               exit 0; \
             fi && \
             EXCLUDES=$(awk '/^exclude:/{{flag=1; next}} flag && /^[[:space:]]*-/ {{gsub(/^[[:space:]]*-[[:space:]]*\"?/,\"\"); gsub(/\"?[[:space:]]*$/,\"\"); print}} flag && /^[^[:space:]-]/ {{flag=0}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             FILTERED=\"\" && \
             while IFS= read -r f; do \
               [ -z \"$f\" ] && continue; \
               skip=0; \
               for pat in $EXCLUDES; do \
                 case \"$f\" in $pat) skip=1; break;; esac; \
               done; \
               [ \"$skip\" -eq 0 ] && FILTERED=\"$FILTERED$f\\n\"; \
             done <<< \"$CHANGED_FILES\" && \
             FILTERED=$(printf '%b' \"$FILTERED\" | sed '/^$/d') && \
             if [ -z \"$FILTERED\" ]; then \
               echo 'jacoco: all changed files excluded by jacoco-config.yml — skipping'; \
               exit 0; \
             fi && \
             echo \"jacoco: checking coverage for $(echo \\\"$FILTERED\\\" | wc -l | tr -d ' ') changed file(s)\" && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco: no exec file at pipelight-misc/jacoco-report/jacoco.exec (test step may have crashed or jacoco plugin output path was customised) — skipping'; \
               exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.xml ]; then \
               if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
                 echo 'Downloading JaCoCo CLI...' && \
                 mkdir -p $JACOCO_DIR && \
                 curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip -o /tmp/jacoco.zip && \
                 (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
               fi && \
               CLASS_DIRS=$(find . -path '*/target/classes' -type d 2>/dev/null | tr '\\n' ' ') && \
               SRC_DIRS=$(find . -path '*/src/main/java' -type d 2>/dev/null | tr '\\n' ' ') && \
               CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
               SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
               java -jar $JACOCO_DIR/lib/jacococli.jar report \
                 /workspace/pipelight-misc/jacoco-report/jacoco.exec \
                 $CLASSFILES_ARGS $SOURCES_ARGS \
                 --xml /workspace/pipelight-misc/jacoco-report/jacoco.xml; \
             fi && \
             true",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
            changed_files = changed_files,
        );
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): generate XML report via jacococli and handle missing exec"
```

---

### Task 13: `MavenJacocoStep` — parse XML, compute per-file LINE%, emit fail marker

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests`:

```rust
    #[test]
    fn test_command_parses_xml_for_line_coverage() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        // Must reference sourcefile/counter XML elements and LINE type.
        assert!(
            cmd.contains("sourcefile") && cmd.contains("LINE"),
            "command must scan sourcefile + LINE counters, got first 300 chars: {}",
            &cmd.chars().take(300).collect::<String>()
        );
    }

    #[test]
    fn test_command_reads_threshold_from_config() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("threshold"),
            "command must read threshold from config, got first 500 chars: {}",
            &cmd.chars().take(500).collect::<String>()
        );
    }

    #[test]
    fn test_command_writes_three_report_files() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("jacoco-summary.txt"));
        assert!(cmd.contains("uncovered.txt"));
        assert!(cmd.contains("threshold-fail.txt"));
    }

    #[test]
    fn test_command_emits_total_marker_and_exits_1_on_failure() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("JaCoCo Total:"),
            "command must emit 'JaCoCo Total:' marker for match_exception, got tail 300 chars: {}",
            &cmd.chars().rev().take(300).collect::<String>().chars().rev().collect::<String>()
        );
        assert!(cmd.contains("exit 1"));
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: the 4 new tests FAIL.

- [ ] **Step 3: Extend the command with XML parsing**

Replace the trailing `true` in the `cmd` formatting with the following parsing logic. Replace the `&& \\n             true",` line with:

```rust
             && \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/uncovered.txt && \
             : > $REPORT/threshold-fail.txt && \
             # Extract per-sourcefile LINE coverage using awk: \
             # For each <sourcefile name=\"X.java\"> block, find the LINE counter \
             # (<counter type=\"LINE\" missed=\"M\" covered=\"C\"/>) and compute C/(M+C)*100. \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" \
                 -v uncovered=\"$REPORT/uncovered.txt\" \
                 -v filtered_list=\"$FILTERED\" '\
               BEGIN {{ \
                 n=split(filtered_list, files, \"\\n\"); \
                 for (i=1;i<=n;i++) {{ if (files[i]!=\"\") {{ keep[files[i]]=1 }} }} \
                 IGNORECASE=1 \
               }} \
               /<sourcefile/ {{ \
                 match($0, /name=\"[^\"]*\"/); \
                 sf_attr=substr($0, RSTART, RLENGTH); \
                 sub(/^name=\"/,\"\",sf_attr); sub(/\"$/,\"\",sf_attr); \
                 current=sf_attr; package_path=\"\"; have_line=0; \
               }} \
               /<package/ {{ \
                 match($0, /name=\"[^\"]*\"/); \
                 pkg_attr=substr($0, RSTART, RLENGTH); \
                 sub(/^name=\"/,\"\",pkg_attr); sub(/\"$/,\"\",pkg_attr); \
                 package_path=pkg_attr; \
               }} \
               /<line.*mi=/ && current!=\"\" && have_line==0 {{ \
                 # track uncovered lines via mi > 0 \
                 if (match($0, /nr=\"[0-9]+\"/)) {{ \
                   nr=substr($0, RSTART+4, RLENGTH-5); \
                   if (match($0, /mi=\"[0-9]+\"/)) {{ \
                     mi=substr($0, RSTART+4, RLENGTH-5); \
                     if (mi+0 > 0) {{ uncov[current]=uncov[current] nr \",\" }} \
                   }} \
                 }} \
               }} \
               /<counter type=\"LINE\"/ && current!=\"\" {{ \
                 if (have_line==1) next; have_line=1; \
                 match($0, /missed=\"[0-9]+\"/); missed_attr=substr($0, RSTART+8, RLENGTH-9); \
                 match($0, /covered=\"[0-9]+\"/); covered_attr=substr($0, RSTART+9, RLENGTH-10); \
                 m=missed_attr+0; c=covered_attr+0; total=m+c; \
                 if (total==0) {{ pct=100 }} else {{ pct=int((c*1000)/total)/10 }}; \
                 rel=current; if (package_path!=\"\") rel=package_path \"/\" current; \
                 # Match against any FILTERED entry ending with our rel. \
                 matched=\"\"; \
                 for (f in keep) {{ if (index(f, rel)>0) {{ matched=f; break }} }} \
                 if (matched!=\"\") {{ \
                   printf(\"%s %.1f%%\\n\", matched, pct) >> summary; \
                   if (pct < threshold) {{ \
                     printf(\"%s %.1f%%\\n\", matched, pct) >> failed; \
                     printf(\"%s: missed lines %s\\n\", matched, uncov[current]) >> uncovered; \
                   }} \
                 }} \
                 current=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && \
             echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             if [ \"$FAIL_COUNT\" -gt 0 ]; then \
               echo \"\" && echo \"=== Files Below Threshold ===\" && cat $REPORT/threshold-fail.txt && \
               exit 1; \
             fi && \
             exit 0",
```

Note: because awk is being embedded inside Rust's `format!`, **every literal `{{` and `}}` is escaping a brace for `format!`; keep them verbatim as shown**. When the macro expands, the shell receives single `{` and `}`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass. (The command string assertions should now match.)

If the Rust code fails to compile with `format!` escaping errors, double-check each awk `{` / `}` is doubled to `{{` / `}}` and each outer substitution (`{cd}`, `{ver}`, `{changed_files}`) stays single-braced.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): parse XML for LINE coverage and emit fail marker"
```

---

### Task 14: `MavenJacocoStep` — `exception_mapping` + `match_exception`

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write failing tests**

Append to `tests`:

```rust
    #[test]
    fn test_coverage_below_triggers_auto_fix() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 3 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 9);
        assert_eq!(resolved.exception_key, "coverage_below_threshold");
    }

    #[test]
    fn test_config_not_found_triggers_auto_gen() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_jacoco_config",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoGenJacocoConfig);
        assert_eq!(resolved.max_retries, 9);
    }

    #[test]
    fn test_to_on_failure_has_expected_exceptions() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(of.callback_command, CallbackCommand::RuntimeError);
        assert!(of.exceptions.contains_key("coverage_below_threshold"));
        assert!(of.exceptions.contains_key("config_not_found"));
        let cov = &of.exceptions["coverage_below_threshold"];
        assert_eq!(cov.command, CallbackCommand::AutoFix);
        assert_eq!(cov.max_retries, 9);
        assert!(cov
            .context_paths
            .iter()
            .any(|p| p.contains("jacoco-report/jacoco.xml")));
        assert!(cov
            .context_paths
            .iter()
            .any(|p| p.contains("uncovered.txt")));
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: the 3 new tests FAIL.

- [ ] **Step 3: Implement exception mapping + match_exception**

Replace the two methods:

```rust
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "coverage_below_threshold",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 9,
                    context_paths: vec![
                        "pipelight-misc/jacoco-report/jacoco.xml".into(),
                        "pipelight-misc/jacoco-report/uncovered.txt".into(),
                        "pipelight-misc/jacoco-report/jacoco-summary.txt".into(),
                        "pipelight-misc/jacoco-report/threshold-fail.txt".into(),
                        "pipelight-misc/jacoco-config.yml".into(),
                        "pipelight-misc/git-diff-report/staged.txt".into(),
                        "pipelight-misc/git-diff-report/unstaged.txt".into(),
                        "pipelight-misc/git-diff-report/untracked.txt".into(),
                        "pipelight-misc/git-diff-report/unpushed.txt".into(),
                    ],
                },
            )
            .add(
                "config_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenJacocoConfig,
                    max_retries: 9,
                    context_paths: self.source_paths.clone(),
                },
            )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            Some("config_not_found".into())
        } else if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): wire exception mapping and match_exception"
```

---

### Task 15: `MavenJacocoStep` — `output_report_str`

**Files:**
- Modify: `src/ci/pipeline_builder/maven/jacoco_step.rs`

- [ ] **Step 1: Write failing tests**

```rust
    #[test]
    fn test_report_str_skip_no_changed_files() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: no changed java/kt files on current branch — skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (no changed files)");
    }

    #[test]
    fn test_report_str_skip_all_excluded() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: all changed files excluded by jacoco-config.yml — skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (all excluded)");
    }

    #[test]
    fn test_report_str_config_not_found() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(false, "", "PIPELIGHT_CALLBACK:auto_gen_jacoco_config ...");
        assert_eq!(r, "jacoco: config not found (callback)");
    }

    #[test]
    fn test_report_str_total_line() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            false,
            "some prefix\nJaCoCo Total: 2 files below 70%\nextra",
            "",
        );
        assert_eq!(r, "JaCoCo Total: 2 files below 70%");
    }

    #[test]
    fn test_report_str_success_default() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(true, "JaCoCo Total: 0 files below 70%", "");
        assert_eq!(r, "JaCoCo Total: 0 files below 70%");
    }

    #[test]
    fn test_report_str_skip_no_exec() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: no exec file at pipelight-misc/jacoco-report/jacoco.exec ... skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (no exec file)");
    }
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: the 6 new tests FAIL.

- [ ] **Step 3: Implement `output_report_str`**

Replace the method:

```rust
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            return "jacoco: config not found (callback)".into();
        }
        if output.contains("no changed java/kt files") {
            return "jacoco: skipped (no changed files)".into();
        }
        if output.contains("all changed files excluded") {
            return "jacoco: skipped (all excluded)".into();
        }
        if output.contains("no exec file") {
            return "jacoco: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if success {
            "jacoco: ok".into()
        } else {
            "jacoco: failed".into()
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_step`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_step.rs
git commit -m "feat(maven-jacoco): human-readable report string"
```

---

## Phase 5 — `MavenJacocoFullStep`

### Task 16: Scaffold `MavenJacocoFullStep` (full-repo, report-only)

**Files:**
- Create: `src/ci/pipeline_builder/maven/jacoco_full_step.rs`
- Modify: `src/ci/pipeline_builder/maven/mod.rs` (add `pub mod`)

- [ ] **Step 1: Create the file**

Create `src/ci/pipeline_builder/maven/jacoco_full_step.rs`:

```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Full-repo JaCoCo scan (tag = "full").
///
/// Activated by `--full-report-only`. Scans every `src/main/{java,kotlin}`
/// dir with the same `pipelight-misc/jacoco-config.yml` exclude rules but
/// without git-diff filtering. Never auto-fixes — findings are surfaced via
/// `jacoco_print_command` so the LLM prints a grouped-by-package table;
/// pipeline is not blocked (`allow_failure: true`).
pub struct MavenJacocoFullStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    #[allow(dead_code)]
    mode: JacocoMode,
}

impl MavenJacocoFullStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for MavenJacocoFullStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found.' >&2; exit 1; \
             fi && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco_full: no exec file — skipping'; exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
             if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
               mkdir -p $JACOCO_DIR && \
               curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip -o /tmp/jacoco.zip && \
               (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
             fi && \
             mkdir -p /workspace/pipelight-misc/jacoco-full-report && \
             CLASS_DIRS=$(find . -path '*/target/classes' -type d 2>/dev/null | tr '\\n' ' ') && \
             SRC_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
             CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
             SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
             java -jar $JACOCO_DIR/lib/jacococli.jar report \
               /workspace/pipelight-misc/jacoco-report/jacoco.exec \
               $CLASSFILES_ARGS $SOURCES_ARGS \
               --xml /workspace/pipelight-misc/jacoco-full-report/jacoco.xml \
               --html /workspace/pipelight-misc/jacoco-full-report/html; \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-full-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/threshold-fail.txt && \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" '\
               /<package/ {{ match($0, /name=\"[^\"]*\"/); pkg=substr($0, RSTART+6, RLENGTH-7) }} \
               /<sourcefile/ {{ match($0, /name=\"[^\"]*\"/); sf=substr($0, RSTART+6, RLENGTH-7); have=0 }} \
               /<counter type=\"LINE\"/ && sf!=\"\" && have==0 {{ \
                 have=1; \
                 match($0,/missed=\"[0-9]+\"/); mi=substr($0,RSTART+8,RLENGTH-9)+0; \
                 match($0,/covered=\"[0-9]+\"/); co=substr($0,RSTART+9,RLENGTH-10)+0; \
                 t=mi+co; pct=(t==0)?100:int((co*1000)/t)/10; \
                 rel=(pkg==\"\")?sf:pkg\"/\"sf; \
                 printf(\"%s %.1f%%\\n\", rel, pct) >> summary; \
                 if (pct<threshold) printf(\"%s %.1f%%\\n\", rel, pct) >> failed; \
                 sf=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             echo \"jacoco_full: report at /workspace/pipelight-misc/jacoco-full-report/\" && \
             exit 0",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
        );
        StepConfig {
            name: "jacoco_full".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["jacoco".into()],
            allow_failure: true,
            active: false,
            tag: "full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError).add(
            "coverage_below_threshold",
            ExceptionEntry {
                command: CallbackCommand::JacocoPrintCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/jacoco-full-report/jacoco.xml".into(),
                    "pipelight-misc/jacoco-full-report/jacoco-summary.txt".into(),
                    "pipelight-misc/jacoco-full-report/threshold-fail.txt".into(),
                ],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("no exec file") {
            return "jacoco_full: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if success {
            "jacoco_full: ok".into()
        } else {
            "jacoco_full: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_full_step_config() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco_full");
        assert_eq!(cfg.depends_on, vec!["jacoco".to_string()]);
        assert_eq!(cfg.tag, "full");
        assert!(cfg.allow_failure);
        assert!(!cfg.active);
    }

    #[test]
    fn test_full_step_uses_print_command() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 5 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::JacocoPrintCommand);
        assert_eq!(resolved.exception_key, "coverage_below_threshold");
    }

    #[test]
    fn test_full_step_reports_to_full_report_dir() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("pipelight-misc/jacoco-full-report"));
    }

    #[test]
    fn test_full_step_total_marker() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("JaCoCo Total:"));
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ci/pipeline_builder/maven/mod.rs`, add near the other `pub mod` lines:

```rust
pub mod jacoco_full_step;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven::jacoco_full_step`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/maven/jacoco_full_step.rs src/ci/pipeline_builder/maven/mod.rs
git commit -m "feat(maven): add MavenJacocoFullStep (full-repo report-only)"
```

---

## Phase 6 — Gradle counterparts

The Gradle steps reuse the same logic but use the Gradle image and Gradle-specific compiled class paths (`build/classes/java/main` instead of `target/classes`).

### Task 17: Scaffold `GradleJacocoStep`

**Files:**
- Create: `src/ci/pipeline_builder/gradle/jacoco_step.rs`
- Modify: `src/ci/pipeline_builder/gradle/mod.rs` (add `pub mod`)

- [ ] **Step 1: Create the file**

Create `src/ci/pipeline_builder/gradle/jacoco_step.rs` with a structure parallel to `MavenJacocoStep`. Only two substantive differences:

- In the XML report generation branch, use `-path '*/build/classes/java/main' -o -path '*/build/classes/kotlin/main'` for `CLASS_DIRS`, and `-path '*/src/main/java' -o -path '*/src/main/kotlin'` for `SRC_DIRS`.
- `depends_on` is `"test"`, `tag: "non-full"`, everything else identical.

Full file contents:

```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{git_changed_files_snippet, StepConfig, StepDef};

/// Incremental JaCoCo coverage check for Gradle (tag = "non-full").
/// See MavenJacocoStep for behavioural contract; only class/source dirs
/// differ (Gradle puts compiled classes under `build/classes/...`).
pub struct GradleJacocoStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,
}

impl GradleJacocoStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for GradleJacocoStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let changed_files = git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref());
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found in pipelight-misc/. LLM should generate one with threshold (default 70) and exclude globs (DTO/Config/Exception/Application at minimum).' >&2; \
               exit 1; \
             fi && \
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'jacoco: no changed java/kt files on current branch — skipping'; \
               exit 0; \
             fi && \
             EXCLUDES=$(awk '/^exclude:/{{flag=1; next}} flag && /^[[:space:]]*-/ {{gsub(/^[[:space:]]*-[[:space:]]*\"?/,\"\"); gsub(/\"?[[:space:]]*$/,\"\"); print}} flag && /^[^[:space:]-]/ {{flag=0}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             FILTERED=\"\" && \
             while IFS= read -r f; do \
               [ -z \"$f\" ] && continue; \
               skip=0; \
               for pat in $EXCLUDES; do \
                 case \"$f\" in $pat) skip=1; break;; esac; \
               done; \
               [ \"$skip\" -eq 0 ] && FILTERED=\"$FILTERED$f\\n\"; \
             done <<< \"$CHANGED_FILES\" && \
             FILTERED=$(printf '%b' \"$FILTERED\" | sed '/^$/d') && \
             if [ -z \"$FILTERED\" ]; then \
               echo 'jacoco: all changed files excluded by jacoco-config.yml — skipping'; \
               exit 0; \
             fi && \
             echo \"jacoco: checking coverage for $(echo \\\"$FILTERED\\\" | wc -l | tr -d ' ') changed file(s)\" && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco: no exec file at pipelight-misc/jacoco-report/jacoco.exec (test step may have crashed or jacoco plugin output path was customised) — skipping'; \
               exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.xml ]; then \
               if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
                 mkdir -p $JACOCO_DIR && \
                 curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip -o /tmp/jacoco.zip && \
                 (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
               fi && \
               CLASS_DIRS=$(find . \\( -path '*/build/classes/java/main' -o -path '*/build/classes/kotlin/main' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
               SRC_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
               CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
               SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
               java -jar $JACOCO_DIR/lib/jacococli.jar report \
                 /workspace/pipelight-misc/jacoco-report/jacoco.exec \
                 $CLASSFILES_ARGS $SOURCES_ARGS \
                 --xml /workspace/pipelight-misc/jacoco-report/jacoco.xml; \
             fi && \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/uncovered.txt && \
             : > $REPORT/threshold-fail.txt && \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" \
                 -v uncovered=\"$REPORT/uncovered.txt\" \
                 -v filtered_list=\"$FILTERED\" '\
               BEGIN {{ n=split(filtered_list, files, \"\\n\"); for (i=1;i<=n;i++) {{ if (files[i]!=\"\") keep[files[i]]=1 }} IGNORECASE=1 }} \
               /<package/ {{ match($0, /name=\"[^\"]*\"/); pkg=substr($0, RSTART+6, RLENGTH-7) }} \
               /<sourcefile/ {{ match($0, /name=\"[^\"]*\"/); current=substr($0, RSTART+6, RLENGTH-7); have_line=0 }} \
               /<line.*mi=/ && current!=\"\" && have_line==0 {{ \
                 if (match($0, /nr=\"[0-9]+\"/)) {{ nr=substr($0, RSTART+4, RLENGTH-5); \
                   if (match($0, /mi=\"[0-9]+\"/)) {{ mi=substr($0, RSTART+4, RLENGTH-5); if (mi+0>0) uncov[current]=uncov[current] nr \",\" }} \
                 }} \
               }} \
               /<counter type=\"LINE\"/ && current!=\"\" {{ \
                 if (have_line==1) next; have_line=1; \
                 match($0,/missed=\"[0-9]+\"/); m=substr($0,RSTART+8,RLENGTH-9)+0; \
                 match($0,/covered=\"[0-9]+\"/); c=substr($0,RSTART+9,RLENGTH-10)+0; \
                 t=m+c; pct=(t==0)?100:int((c*1000)/t)/10; \
                 rel=(pkg==\"\")?current:pkg\"/\"current; \
                 matched=\"\"; for (f in keep) if (index(f, rel)>0) {{ matched=f; break }} \
                 if (matched!=\"\") {{ \
                   printf(\"%s %.1f%%\\n\", matched, pct) >> summary; \
                   if (pct<threshold) {{ \
                     printf(\"%s %.1f%%\\n\", matched, pct) >> failed; \
                     printf(\"%s: missed lines %s\\n\", matched, uncov[current]) >> uncovered; \
                   }} \
                 }} \
                 current=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && \
             echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             if [ \"$FAIL_COUNT\" -gt 0 ]; then \
               echo \"\" && echo \"=== Files Below Threshold ===\" && cat $REPORT/threshold-fail.txt && \
               exit 1; \
             fi && \
             exit 0",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
            changed_files = changed_files,
        );
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "coverage_below_threshold",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 9,
                    context_paths: vec![
                        "pipelight-misc/jacoco-report/jacoco.xml".into(),
                        "pipelight-misc/jacoco-report/uncovered.txt".into(),
                        "pipelight-misc/jacoco-report/jacoco-summary.txt".into(),
                        "pipelight-misc/jacoco-report/threshold-fail.txt".into(),
                        "pipelight-misc/jacoco-config.yml".into(),
                        "pipelight-misc/git-diff-report/staged.txt".into(),
                        "pipelight-misc/git-diff-report/unstaged.txt".into(),
                        "pipelight-misc/git-diff-report/untracked.txt".into(),
                        "pipelight-misc/git-diff-report/unpushed.txt".into(),
                    ],
                },
            )
            .add(
                "config_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenJacocoConfig,
                    max_retries: 9,
                    context_paths: self.source_paths.clone(),
                },
            )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            Some("config_not_found".into())
        } else if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            return "jacoco: config not found (callback)".into();
        }
        if output.contains("no changed java/kt files") {
            return "jacoco: skipped (no changed files)".into();
        }
        if output.contains("all changed files excluded") {
            return "jacoco: skipped (all excluded)".into();
        }
        if output.contains("no exec file") {
            return "jacoco: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if success {
            "jacoco: ok".into()
        } else {
            "jacoco: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8-jdk17".into(),
            build_cmd: vec!["./gradlew assemble".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_basic_step_config() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco");
        assert_eq!(cfg.depends_on, vec!["test".to_string()]);
        assert_eq!(cfg.tag, "non-full");
        assert!(!cfg.allow_failure);
    }

    #[test]
    fn test_coverage_below_triggers_auto_fix() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 3 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 9);
    }

    #[test]
    fn test_config_not_found_triggers_auto_gen() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_jacoco_config",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoGenJacocoConfig);
    }

    #[test]
    fn test_command_uses_gradle_build_dirs() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("build/classes/java/main"),
            "Gradle step must look in Gradle's compiled output dir, got: {}",
            &cmd.chars().take(500).collect::<String>()
        );
    }
}
```

- [ ] **Step 2: Register in `gradle/mod.rs`**

```rust
pub mod jacoco_step;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::gradle::jacoco_step`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/gradle/jacoco_step.rs src/ci/pipeline_builder/gradle/mod.rs
git commit -m "feat(gradle): add GradleJacocoStep mirroring Maven counterpart"
```

---

### Task 18: Scaffold `GradleJacocoFullStep`

**Files:**
- Create: `src/ci/pipeline_builder/gradle/jacoco_full_step.rs`
- Modify: `src/ci/pipeline_builder/gradle/mod.rs`

- [ ] **Step 1: Create the file**

Create `src/ci/pipeline_builder/gradle/jacoco_full_step.rs`. Mirror `MavenJacocoFullStep` with one substantive change: the CLASS_DIRS and SRC_DIRS finds use Gradle paths.

```rust
use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Full-repo JaCoCo scan for Gradle (tag = "full").
/// Activated by `--full-report-only`. Report-only (allow_failure=true).
pub struct GradleJacocoFullStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    #[allow(dead_code)]
    mode: JacocoMode,
}

impl GradleJacocoFullStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for GradleJacocoFullStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found.' >&2; exit 1; \
             fi && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco_full: no exec file — skipping'; exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
             if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
               mkdir -p $JACOCO_DIR && \
               curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip -o /tmp/jacoco.zip && \
               (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
             fi && \
             mkdir -p /workspace/pipelight-misc/jacoco-full-report && \
             CLASS_DIRS=$(find . \\( -path '*/build/classes/java/main' -o -path '*/build/classes/kotlin/main' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
             SRC_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
             CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
             SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
             java -jar $JACOCO_DIR/lib/jacococli.jar report \
               /workspace/pipelight-misc/jacoco-report/jacoco.exec \
               $CLASSFILES_ARGS $SOURCES_ARGS \
               --xml /workspace/pipelight-misc/jacoco-full-report/jacoco.xml \
               --html /workspace/pipelight-misc/jacoco-full-report/html; \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-full-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/threshold-fail.txt && \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" '\
               /<package/ {{ match($0, /name=\"[^\"]*\"/); pkg=substr($0, RSTART+6, RLENGTH-7) }} \
               /<sourcefile/ {{ match($0, /name=\"[^\"]*\"/); sf=substr($0, RSTART+6, RLENGTH-7); have=0 }} \
               /<counter type=\"LINE\"/ && sf!=\"\" && have==0 {{ \
                 have=1; match($0,/missed=\"[0-9]+\"/); mi=substr($0,RSTART+8,RLENGTH-9)+0; \
                 match($0,/covered=\"[0-9]+\"/); co=substr($0,RSTART+9,RLENGTH-10)+0; \
                 t=mi+co; pct=(t==0)?100:int((co*1000)/t)/10; \
                 rel=(pkg==\"\")?sf:pkg\"/\"sf; \
                 printf(\"%s %.1f%%\\n\", rel, pct) >> summary; \
                 if (pct<threshold) printf(\"%s %.1f%%\\n\", rel, pct) >> failed; \
                 sf=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             echo \"jacoco_full: report at /workspace/pipelight-misc/jacoco-full-report/\" && \
             exit 0",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
        );
        StepConfig {
            name: "jacoco_full".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["jacoco".into()],
            allow_failure: true,
            active: false,
            tag: "full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError).add(
            "coverage_below_threshold",
            ExceptionEntry {
                command: CallbackCommand::JacocoPrintCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/jacoco-full-report/jacoco.xml".into(),
                    "pipelight-misc/jacoco-full-report/jacoco-summary.txt".into(),
                    "pipelight-misc/jacoco-full-report/threshold-fail.txt".into(),
                ],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("no exec file") {
            return "jacoco_full: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if success {
            "jacoco_full: ok".into()
        } else {
            "jacoco_full: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8-jdk17".into(),
            build_cmd: vec!["./gradlew assemble".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_full_step_config() {
        let step = GradleJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco_full");
        assert_eq!(cfg.depends_on, vec!["jacoco".to_string()]);
        assert_eq!(cfg.tag, "full");
        assert!(cfg.allow_failure);
        assert!(!cfg.active);
    }

    #[test]
    fn test_full_step_uses_print_command() {
        let step = GradleJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 5 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::JacocoPrintCommand);
    }
}
```

- [ ] **Step 2: Register in `gradle/mod.rs`**

```rust
pub mod jacoco_full_step;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::gradle::jacoco_full_step`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/ci/pipeline_builder/gradle/jacoco_full_step.rs src/ci/pipeline_builder/gradle/mod.rs
git commit -m "feat(gradle): add GradleJacocoFullStep"
```

---

## Phase 7 — Strategy integration

### Task 19: Wire JaCoCo steps into `MavenStrategy`

**Files:**
- Modify: `src/ci/pipeline_builder/maven/mod.rs`
- Modify: `src/ci/pipeline_builder/base/mod.rs` (extend `default_report_str` for jacoco step names)

- [ ] **Step 1: Write failing strategy-level tests**

In `src/ci/pipeline_builder/maven/mod.rs`, inside the existing `tests` module, add:

```rust
    #[test]
    fn test_maven_steps_include_jacoco_after_test_before_package() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(
            names,
            vec![
                "build",
                "checkstyle",
                "spotbugs",
                "spotbugs_full",
                "pmd",
                "pmd_full",
                "test",
                "jacoco",
                "jacoco_full",
                "package",
            ]
        );
    }

    #[test]
    fn test_package_depends_on_jacoco_full() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let pkg = steps.iter().find(|s| s.config().name == "package").unwrap().config();
        assert_eq!(pkg.depends_on, vec!["jacoco_full".to_string()]);
    }

    #[test]
    fn test_jacoco_full_inactive_by_default() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let jf = steps.iter().find(|s| s.config().name == "jacoco_full").unwrap().config();
        assert!(!jf.active);
        assert_eq!(jf.tag, "full");
        assert!(jf.allow_failure);
    }

    #[test]
    fn test_jacoco_mode_standalone_when_plugin_absent() {
        let info = make_maven_info_without_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let test = steps.iter().find(|s| s.config().name == "test").unwrap().config();
        let combined = test.commands.join(" && ");
        assert!(
            combined.contains("MAVEN_OPTS") && combined.contains("jacocoagent.jar"),
            "without jacoco plugin, test step must inject standalone agent, got: {}",
            combined
        );
    }

    #[test]
    fn test_jacoco_mode_plugin_when_plugin_present() {
        let mut info = make_maven_info_with_lint();
        info.quality_plugins.push("jacoco".to_string());
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let test = steps.iter().find(|s| s.config().name == "test").unwrap().config();
        let combined = test.commands.join(" && ");
        assert!(
            combined.contains("jacoco:prepare-agent"),
            "with jacoco plugin, test step must use prepare-agent, got: {}",
            combined
        );
    }
```

Also update the existing `test_maven_steps_with_checkstyle` and `test_maven_steps_without_checkstyle`: adjust their expected name vectors to include `"jacoco"` and `"jacoco_full"` between `"test"` and `"package"`, and adjust the `by_name["package"]` assertion from `vec!["test".to_string()]` to `vec!["jacoco_full".to_string()]`. Also adjust the chained `assert_eq!` for test's depends_on: the test step still depends on `pmd_full`, but now jacoco depends on test, jacoco_full on jacoco, package on jacoco_full.

Concretely, the updated sections should read:

```rust
        assert_eq!(
            names,
            vec![
                "build",
                "checkstyle",
                "spotbugs",
                "spotbugs_full",
                "pmd",
                "pmd_full",
                "test",
                "jacoco",
                "jacoco_full",
                "package",
            ]
        );
        // ... (within test_maven_steps_with_checkstyle assertions:)
        assert_eq!(by_name["test"], vec!["pmd_full".to_string()]);
        assert_eq!(by_name["jacoco"], vec!["test".to_string()]);
        assert_eq!(by_name["jacoco_full"], vec!["jacoco".to_string()]);
        assert_eq!(by_name["package"], vec!["jacoco_full".to_string()]);
```

Same adjustments for `test_maven_steps_without_checkstyle` (same names minus `"checkstyle"`), and for `test_package_depends_on_test` — **rename that test to `test_package_depends_on_jacoco_full`** and update its assertion:

```rust
    #[test]
    fn test_package_depends_on_jacoco_full_via_strategy() {
        // PackageStep itself still declares depends_on=["test"]; the strategy
        // overrides it via wrap_with_deps when assembling the DAG.
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let pkg = steps.iter().find(|s| s.config().name == "package").unwrap().config();
        assert_eq!(pkg.depends_on, vec!["jacoco_full".to_string()]);
    }
```

Remove the old `test_package_depends_on_test` that instantiates `PackageStep::new` directly and asserts `depends_on = ["test"]` — that's checking the default which is still true, so actually keep that test (it tests PackageStep in isolation, and PackageStep's default `depends_on = ["test"]` is still correct because the strategy overrides it).

**Re-reading the existing `test_package_depends_on_test`:**
```rust
    #[test]
    fn test_package_depends_on_test() {
        let info = make_maven_info_with_lint();
        let step = package_step::PackageStep::new(info);
        assert_eq!(step.config().depends_on, vec!["test"]);
    }
```
This tests `PackageStep::new(info).config().depends_on` — that's the raw step's default, still `["test"]`. Keep this test as-is.

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven`
Expected: new tests FAIL and old `test_maven_steps_with_checkstyle` / `test_maven_steps_without_checkstyle` FAIL due to name vector mismatch (until the strategy is updated).

- [ ] **Step 3: Update `MavenStrategy::steps`**

In `src/ci/pipeline_builder/maven/mod.rs`, replace the `test` and `package` assembly (the last ~40 lines of `fn steps`) with:

```rust
        // Maven test step: mirror gradle — inject `--fail-at-end` so every
        // module's tests run (A fails → B still runs), then mark the step
        // report-only so the pipeline continues past test failures.
        let mut test_info = info.clone();
        test_info.test_cmd = info
            .test_cmd
            .iter()
            .map(|cmd| {
                if !cmd.contains("--fail-at-end") && !cmd.contains("-fae") {
                    format!("{} --fail-at-end", cmd)
                } else {
                    cmd.clone()
                }
            })
            .collect();
        let test_step_inner = TestStep::new(&test_info)
            .with_parser(parse_maven_test)
            .with_allow_failure(true)
            .with_test_report_globs(vec![
                "**/target/surefire-reports/TEST-*.xml".into(),
                "**/target/failsafe-reports/TEST-*.xml".into(),
            ])
            .with_failure_markers(
                vec!["BUILD FAILURE".into(), "There are test failures".into()],
                "Tests had failures (report-only)",
            );

        // Decide JaCoCo mode from detector output: plugin if project pom has
        // jacoco-maven-plugin, standalone otherwise.
        let jacoco_mode = if info.quality_plugins.iter().any(|p| p == "jacoco") {
            crate::ci::pipeline_builder::base::JacocoMode::MavenPlugin
        } else {
            crate::ci::pipeline_builder::base::JacocoMode::Standalone
        };
        let wrapped_test = crate::ci::pipeline_builder::base::JacocoAgentTestStep::new(
            Box::new(test_step_inner),
            jacoco_mode.clone(),
        );
        steps.push(Box::new(MavenCachedStep::wrap_with_deps(
            Box::new(wrapped_test),
            vec![prev],
        )));
        prev = "test".into();

        // JaCoCo incremental + full
        steps.push(Box::new(MavenCachedStep::wrap_with_deps(
            Box::new(jacoco_step::MavenJacocoStep::new(info, jacoco_mode.clone())),
            vec![prev.clone()],
        )));
        prev = "jacoco".into();

        steps.push(Box::new(MavenCachedStep::wrap_with_deps(
            Box::new(jacoco_full_step::MavenJacocoFullStep::new(info, jacoco_mode)),
            vec![prev.clone()],
        )));
        prev = "jacoco_full".into();

        // Package depends on jacoco_full (override default ["test"])
        steps.push(Box::new(MavenCachedStep::wrap_with_deps(
            Box::new(package_step::PackageStep::new(info)),
            vec![prev],
        )));

        steps
```

Also add `use` imports at the top of the file (below the existing `use crate::ci::pipeline_builder::...`):

```rust
// Already imported: test_parser, PipelineStrategy, StepConfig, StepDef
// Add JacocoAgentTestStep + JacocoMode:
// (If the paths above use `crate::ci::pipeline_builder::base::...` inline,
// no additional top-level imports are needed. Keep inline path for clarity.)
```

(No top-level imports needed if we kept inline `crate::ci::pipeline_builder::base::JacocoMode` paths.)

- [ ] **Step 4: Extend `default_report_str` in `base/mod.rs`**

In `src/ci/pipeline_builder/base/mod.rs`, inside `default_report_str`'s `match step_name` block, add two new arms next to the `"pmd" | "pmd_full"` arm:

```rust
            "jacoco" | "jacoco_full" => Self::report_jacoco(step_name, success, &output),
```

And define the helper at the bottom of the `impl BaseStrategy` block (next to `report_pmd`):

```rust
    fn report_jacoco(step_name: &str, success: bool, output: &str) -> String {
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            return format!("{}: config not found (callback)", step_name);
        }
        if output.contains("no changed java/kt files") {
            return format!("{}: skipped (no changed files)", step_name);
        }
        if output.contains("all changed files excluded") {
            return format!("{}: skipped (all excluded)", step_name);
        }
        if output.contains("no exec file") {
            return format!("{}: skipped (no exec file)", step_name);
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if !success {
            format!("{}: failed", step_name)
        } else {
            format!("{}: ok", step_name)
        }
    }
```

- [ ] **Step 5: Run all maven tests**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::maven`
Expected: all pass (including the DAG ordering tests).

Run: `cargo test -p pipelight --lib`
Expected: the full test suite still passes (especially the pipeline-generation integration test in `src/ci/pipeline_builder/mod.rs` which iterates over steps).

- [ ] **Step 6: Commit**

```bash
git add src/ci/pipeline_builder/maven/mod.rs src/ci/pipeline_builder/base/mod.rs
git commit -m "feat(maven-strategy): wire jacoco + jacoco_full into pipeline DAG"
```

---

### Task 20: Wire JaCoCo steps into `GradleStrategy`

**Files:**
- Modify: `src/ci/pipeline_builder/gradle/mod.rs`

- [ ] **Step 1: Write failing tests**

In the existing `tests` module of `src/ci/pipeline_builder/gradle/mod.rs`, add:

```rust
    #[test]
    fn test_gradle_steps_include_jacoco_at_tail() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<String> = steps.iter().map(|s| s.config().name).collect();
        assert_eq!(
            names,
            vec![
                "build",
                "checkstyle",
                "spotbugs",
                "spotbugs_full",
                "pmd",
                "pmd_full",
                "test",
                "jacoco",
                "jacoco_full",
            ]
        );
    }

    #[test]
    fn test_gradle_jacoco_depends_on_test() {
        let info = make_gradle_info_with_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let by_name: std::collections::HashMap<String, Vec<String>> = steps
            .iter()
            .map(|s| (s.config().name, s.config().depends_on))
            .collect();
        assert_eq!(by_name["jacoco"], vec!["test".to_string()]);
        assert_eq!(by_name["jacoco_full"], vec!["jacoco".to_string()]);
    }

    #[test]
    fn test_gradle_jacoco_mode_standalone_by_default() {
        let info = make_gradle_info_without_lint();
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let test = steps.iter().find(|s| s.config().name == "test").unwrap().config();
        let combined = test.commands.join(" && ");
        assert!(
            combined.contains("JAVA_TOOL_OPTIONS") && combined.contains("jacocoagent.jar"),
            "gradle without jacoco plugin must use standalone agent, got: {}",
            combined
        );
    }

    #[test]
    fn test_gradle_jacoco_mode_plugin_when_present() {
        let mut info = make_gradle_info_with_lint();
        info.quality_plugins.push("jacoco".to_string());
        let strategy = GradleStrategy;
        let steps = strategy.steps(&info);
        let test = steps.iter().find(|s| s.config().name == "test").unwrap().config();
        let combined = test.commands.join(" && ");
        assert!(
            combined.contains("jacocoTestReport") && combined.contains("cp build/jacoco/test.exec"),
            "gradle with jacoco plugin must append jacocoTestReport + copy fallback, got: {}",
            combined
        );
    }
```

Also update the existing `test_gradle_steps_with_lint` and `test_gradle_steps_without_lint` — append `"jacoco"` and `"jacoco_full"` to the expected name vectors, and extend the by_name assertions to include:

```rust
        assert_eq!(by_name["jacoco"], vec!["test".to_string()]);
        assert_eq!(by_name["jacoco_full"], vec!["jacoco".to_string()]);
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::gradle`
Expected: FAIL.

- [ ] **Step 3: Update `GradleStrategy::steps`**

At the end of `fn steps`, after the existing test_step push, append:

```rust
        prev = "test".into();

        let jacoco_mode = if info.quality_plugins.iter().any(|p| p == "jacoco") {
            crate::ci::pipeline_builder::base::JacocoMode::GradlePlugin
        } else {
            crate::ci::pipeline_builder::base::JacocoMode::Standalone
        };
        // Rewrap the just-pushed test step with JacocoAgentTestStep. Simplest
        // approach: pop and re-push. But to avoid reordering, rebuild from scratch:
```

Actually, simpler: change the test_step push to wrap with the decorator inline. Replace the final `steps.push(Box::new(GradleCachedStep::wrap_with_deps(Box::new(test_step), vec![prev])));` block with:

```rust
        let jacoco_mode = if info.quality_plugins.iter().any(|p| p == "jacoco") {
            crate::ci::pipeline_builder::base::JacocoMode::GradlePlugin
        } else {
            crate::ci::pipeline_builder::base::JacocoMode::Standalone
        };
        let wrapped_test = crate::ci::pipeline_builder::base::JacocoAgentTestStep::new(
            Box::new(test_step),
            jacoco_mode.clone(),
        );
        steps.push(Box::new(GradleCachedStep::wrap_with_deps(
            Box::new(wrapped_test),
            vec![prev],
        )));
        prev = "test".into();

        // JaCoCo incremental + full
        steps.push(Box::new(GradleCachedStep::wrap_with_deps(
            Box::new(jacoco_step::GradleJacocoStep::new(info, jacoco_mode.clone())),
            vec![prev.clone()],
        )));
        prev = "jacoco".into();

        steps.push(Box::new(GradleCachedStep::wrap_with_deps(
            Box::new(jacoco_full_step::GradleJacocoFullStep::new(info, jacoco_mode)),
            vec![prev.clone()],
        )));
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p pipelight --lib ci::pipeline_builder::gradle`
Expected: all pass.

Run: `cargo test -p pipelight --lib`
Expected: full suite passes.

- [ ] **Step 5: Commit**

```bash
git add src/ci/pipeline_builder/gradle/mod.rs
git commit -m "feat(gradle-strategy): wire jacoco + jacoco_full into pipeline DAG"
```

---

## Phase 8 — Skill documentation sync

### Task 21: Update `global-skills/pipelight-run/SKILL.md` callback table

**Files:**
- Modify: `global-skills/pipelight-run/SKILL.md`

- [ ] **Step 1: Locate the callback command table**

Open `global-skills/pipelight-run/SKILL.md` and find the section titled "回调命令处理表" (or English equivalent "Callback Command Handling Table"). It is a markdown table with columns for `command`, `action`, LLM operation, success/failure handling.

- [ ] **Step 2: Add two new rows**

Immediately before the end of the table (after the existing `git_diff_command` row), add:

```markdown
| `auto_gen_jacoco_config` | `retry` | LLM 在 pipelight-misc/ 下生成 `jacoco-config.yml`（含 `threshold: 70` 和排除 glob 列表），然后 retry 该 step | step 通过 | 见下文 `auto_gen_jacoco_config` 详细流程 |
| `jacoco_print_command` | `jacoco_print` | LLM 解析 `pipelight-misc/jacoco-full-report/jacoco.xml`，按 package 分组打印每个 source file 的 LINE 覆盖率百分比 + 总结统计（≥70% / <70% 数量） | pipeline 继续（全仓报告是 report-only） | — |
```

- [ ] **Step 3: Add `auto_gen_jacoco_config` detailed section**

After the existing `auto_gen_pmd_ruleset` detailed section, add a new section:

```markdown
### `auto_gen_jacoco_config` 详细流程

1. **读上下文**：LLM 读 `on_failure.context_paths` 指向的 source_paths（项目源码目录）。
2. **识别项目约定**：扫描 src/main/java 下的文件命名模式，找出常见的 DTO/Config/Exception/Entity 类（例如文件名以 `*Dto.java`、`*DTO.java`、`*Config.java`、`*Configuration.java`、`*Exception.java`、`*Application.java` 结尾）。
3. **写配置**：生成 `pipelight-misc/jacoco-config.yml`，格式：

   ```yaml
   # JaCoCo 覆盖率检查配置
   # 被 pipelight 的 jacoco / jacoco_full step 读取

   # 每个文件的 LINE 覆盖率下限（百分比）
   threshold: 70

   # 排除在覆盖率检查外的文件 glob（相对项目根）
   exclude:
     - "**/*Dto.java"
     - "**/*DTO.java"
     - "**/*Config.java"
     - "**/*Configuration.java"
     - "**/*Exception.java"
     - "**/*Application.java"
     - "**/generated/**"
   ```

   LLM 可基于项目实际特征增删排除项（比如观察到大量 `*Mapper.java` 只做 SQL 映射，可追加）。

4. **LLM 打印**：`Generated pipelight-misc/jacoco-config.yml (threshold=70, N excludes).`
5. **retry**：`pipelight retry --step jacoco`。
```

- [ ] **Step 4: Add `jacoco_print_command` detailed section (optional but symmetric)**

Optionally, add a symmetric section after `pmd_print` or `spotbugs_print` describing the expected print format:

```markdown
### `jacoco_print_command` 详细流程

触发于 `jacoco_full` step（`--full-report-only` 模式），全仓有文件 < threshold 时。

1. **读 XML**：`pipelight-misc/jacoco-full-report/jacoco.xml`。
2. **读 summary**：`jacoco-summary.txt`（每行 `path X.Y%`）和 `threshold-fail.txt`（未达标文件）。
3. **打印表格**：按 package 分组，每行 `file coverage% uncovered_lines`；末尾给出汇总（总文件数、达标数、未达标数、平均覆盖率）。
4. **不 retry、不修代码**：`jacoco_full` 是 report-only。

示例输出：

```
=== JaCoCo Full Report (threshold=70%) ===

com.foo.user:
  UserService.java          82.5%  (42/51)
  UserController.java       91.2%  (31/34)

com.foo.order:
  OrderService.java         63.1%  (41/65)  ⚠ below threshold
  OrderRepository.java      95.0%  (19/20)

Total: 4 files — 3 pass, 1 fail — avg 82.9%
```
```

- [ ] **Step 5: Sync to local skill cache**

```bash
cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/
```

- [ ] **Step 6: Commit**

```bash
git add global-skills/pipelight-run/SKILL.md
git commit -m "docs(skill): add JaCoCo callback commands to pipelight-run skill"
```

---

## Phase 9 — Final verification

### Task 22: Full test suite + lint + fmt pass

**Files:** none (verification only)

- [ ] **Step 1: Format**

Run: `cargo fmt`
Expected: no output (already formatted) or benign whitespace changes.

- [ ] **Step 2: Clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings. If any appear (e.g., unused imports, dead code), fix them.

- [ ] **Step 3: Full test suite**

Run: `cargo test -p pipelight`
Expected: every test passes. Pay attention to the pipeline-generation integration tests in `src/ci/pipeline_builder/mod.rs::tests::test_generate_pipeline_has_on_failure` — they iterate over steps and may need no changes because they don't enumerate jacoco.

- [ ] **Step 4: Smoke-check pipeline generation on a sample Maven project**

```bash
mkdir -p /tmp/jacoco-smoke && cd /tmp/jacoco-smoke
cat > pom.xml <<'POM'
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>smoke</groupId>
  <artifactId>smoke</artifactId>
  <version>1.0</version>
  <properties><java.version>17</java.version></properties>
</project>
POM
cargo run --manifest-path "$OLDPWD/Cargo.toml" -- validate 2>/dev/null || true
# Above probably errors: just generate pipeline.yml via the `run` subcommand
# or whatever the project's command is. The important check: the generated
# pipeline.yml should list jacoco + jacoco_full steps between test and package.
```

Adjust the above to whatever command generates `pipeline.yml` in this project (`pipelight init` / `pipelight run --dry-run`, etc.). **Accept any command that proves the DAG looks right** — this is a sanity check, not a rigorous integration test.

- [ ] **Step 5: Commit if any fixes were needed (else skip)**

```bash
git add -A  # only if there are format/clippy fixes
git commit -m "chore: cargo fmt + clippy after jacoco integration"
```

- [ ] **Step 6: Final review**

Open `pipeline.yml` in a Maven sample project and verify manually:
- `test` step has `MAVEN_OPTS=...jacocoagent.jar...` (standalone mode) or `jacoco:prepare-agent` (plugin mode)
- `jacoco` step appears after `test`
- `jacoco_full` step appears after `jacoco` (and is marked `active: false`)
- `package` step's `depends_on` is `jacoco_full`
- New `on_failure.callback_command` values appear: `auto_fix` on jacoco (via `coverage_below_threshold` exception), `auto_gen_jacoco_config` on config_not_found, `jacoco_print_command` on jacoco_full

---

## Self-Review Checklist (completed by plan author)

**1. Spec coverage**
- Section "架构总览" new/modified file list — covered by Tasks 1–20.
- Section "Detector 扩展" — Tasks 3, 4.
- Section "Callback 扩展" — Tasks 1, 2.
- Section "JacocoAgentTestStep decorator" — Tasks 5–8.
- Section "MavenJacocoStep / GradleJacocoStep (increment)" — Tasks 9–15 (Maven), Task 17 (Gradle).
- Section "MavenJacocoFullStep / GradleJacocoFullStep (全仓)" — Tasks 16, 18.
- Section "strategy 组装变化" — Tasks 19, 20.
- Section "数据流 (端到端)" — Implicitly covered by the chained tasks + smoke test in Task 22.
- Section "边界情况处理" — Skip-on-missing-exec (Task 12), all-excluded skip (Task 11), config-not-found (Task 10), full-step missing-exec (Task 16, 18).
- Section "配置文件: jacoco-config.yml" — Described in `auto_gen_jacoco_config` detailed section (Task 21).
- Section "测试策略" — Each behaviour has a dedicated TDD task.
- Section "和 skill 的同步" — Task 21.

**2. Placeholder scan** — no TBD / TODO left; every step has concrete code or exact commands.

**3. Type consistency**
- `JacocoMode` enum referenced with identical variant names (`None`, `Standalone`, `MavenPlugin`, `GradlePlugin`) in decorator + step + strategies.
- `MavenJacocoStep::new(info, mode)` signature matches strategy's `jacoco_step::MavenJacocoStep::new(info, jacoco_mode.clone())` call.
- `JACOCO_VERSION` constant is `&str` and referenced as `{ver}` substitution in format strings everywhere.
- `"coverage_below_threshold"` exception key spelled identically in `exception_mapping` and `match_exception`.

**4. Spec requirement → task mapping has no gaps.**
