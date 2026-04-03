# Step Strategy Architecture Design

## Summary

Introduce a strategy module that decouples "how to run CI" from "what kind of project is this." Each language gets its own strategy that inherits standard steps from a shared base and can override or extend them with language-specific steps. Each custom step lives in its own Rust file.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Standard steps + extension | Base provides build/test/lint/fmt; languages can override and add custom steps | Covers 90% with defaults, 10% with customization |
| Default on_failure in base | Base defines sensible defaults (build=AutoFix(3), test=Notify); languages can override per-step | Eliminates boilerplate while allowing fine-tuning |
| File granularity | Common steps in base/mod.rs; language-specific steps each get their own file | Easy to add new steps without touching existing code |
| Detector stays independent | Detector answers "what is this project"; strategy answers "how to CI it" | Detector logic is stable and well-tested; no reason to rewrite |

## Architecture

### Data Flow

```
detector.detect(dir)
    |
    v
detector.analyze(dir) --> ProjectInfo
                              |
                              v
               strategy.steps(&info) --> Vec<StepDef>
                                             |
                                             v
                                        Pipeline { steps }
                                             |
                                             v
                                        pipeline.yml
```

### Directory Structure

```
src/strategy/
  mod.rs                  -> PipelineStrategy trait, StepDef struct, registry, dispatch
  base/
    mod.rs                -> BaseStrategy: build_step, test_step, lint_step, fmt_step
  maven/
    mod.rs                -> MavenStrategy impl, wires base + custom steps
    checkstyle.rs         -> mvn checkstyle:check
    package.rs            -> mvn package -DskipTests
  gradle/
    mod.rs                -> GradleStrategy impl
    checkstyle.rs         -> ./gradlew check -x test
  rust/
    mod.rs                -> RustStrategy impl
    clippy.rs             -> cargo clippy -- -D warnings
  node/
    mod.rs                -> NodeStrategy impl
    typecheck.rs          -> tsc --noEmit (TypeScript projects)
  python/
    mod.rs                -> PythonStrategy impl
    mypy.rs               -> mypy type checking
  go/
    mod.rs                -> GoStrategy impl
    vet.rs                -> go vet ./...
```

### Core Types

```rust
// src/strategy/mod.rs

/// Definition of a single pipeline step, produced by strategies.
pub struct StepDef {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,               // default: "/workspace"
    pub on_failure: Option<OnFailure>,
    pub allow_failure: bool,
}

/// All language strategies implement this trait.
pub trait PipelineStrategy {
    /// Return all steps for this project (base + language-specific).
    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef>;

    /// Pipeline name (e.g., "maven-java-ci").
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
}
```

### BaseStrategy

`BaseStrategy` is a plain struct with associated functions (not a trait). It provides factory methods for the four standard steps. Language strategies call these directly and can modify the returned `StepDef` before including it.

```rust
// src/strategy/base/mod.rs

impl BaseStrategy {
    /// Standard build step. AutoFix with 3 retries.
    /// Commands from info.build_cmd, context from info.source_paths + info.config_files.
    pub fn build_step(info: &ProjectInfo) -> StepDef;

    /// Standard test step. Depends on "build". Notify on failure.
    /// Commands from info.test_cmd.
    pub fn test_step(info: &ProjectInfo) -> StepDef;

    /// Standard lint step. Returns None if info.lint_cmd is None.
    /// AutoFix with 2 retries.
    pub fn lint_step(info: &ProjectInfo) -> Option<StepDef>;

    /// Standard fmt-check step. Returns None if info.fmt_cmd is None.
    /// AutoFix with 1 retry.
    pub fn fmt_step(info: &ProjectInfo) -> Option<StepDef>;
}
```

### Language Strategy Example: Maven

```rust
// src/strategy/maven/mod.rs

mod checkstyle;
mod package;

pub struct MavenStrategy;

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![
            BaseStrategy::build_step(info),
            BaseStrategy::test_step(info),
        ];

        // Override: Maven build gets more retries
        steps[0].on_failure = Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 5,
            context_paths: vec!["pom.xml".into()],
        });

        // Conditional: add checkstyle if plugin is present
        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
        }

        // Always: add package step
        steps.push(package::step(info));

        steps
    }
}
```

### Custom Step File Pattern

Each language-specific step is a single file exporting a `step()` function:

```rust
// src/strategy/maven/checkstyle.rs

use crate::strategy::StepDef;
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "checkstyle".into(),
        image: info.image.clone(),
        commands: vec!["mvn checkstyle:check".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: vec!["pom.xml".into()],
        }),
        ..Default::default()
    }
}
```

### Strategy Registry

Maps `ProjectType` (from detector) to the corresponding strategy:

```rust
// src/strategy/mod.rs

fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        ProjectType::Maven  => Box::new(maven::MavenStrategy),
        ProjectType::Gradle => Box::new(gradle::GradleStrategy),
        ProjectType::Rust   => Box::new(rust::RustStrategy),
        ProjectType::Node   => Box::new(node::NodeStrategy),
        ProjectType::Python => Box::new(python::PythonStrategy),
        ProjectType::Go     => Box::new(go::GoStrategy),
    }
}

/// Called from cmd_init. Replaces the current generate_pipeline().
pub fn generate_pipeline(info: &ProjectInfo) -> Pipeline {
    let strategy = strategy_for(&info.project_type);
    let step_defs = strategy.steps(info);
    let name = strategy.pipeline_name(info);

    Pipeline {
        name,
        env: HashMap::new(),
        steps: step_defs.into_iter().map(|sd| sd.into()).collect(),
    }
}
```

### StepDef to Step Conversion

`StepDef` converts to the existing `pipeline::Step` via `From`:

```rust
impl From<StepDef> for Step {
    fn from(sd: StepDef) -> Self {
        Step {
            name: sd.name,
            image: sd.image,
            commands: sd.commands,
            depends_on: sd.depends_on,
            workdir: sd.workdir,
            on_failure: sd.on_failure,
            allow_failure: sd.allow_failure,
            env: HashMap::new(),
            condition: None,
        }
    }
}
```

## How to Add a New Step

Example: add a company compliance check to Maven.

**1. Create** `src/strategy/maven/compliance.rs`:

```rust
pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "compliance-check".into(),
        image: info.image.clone(),
        commands: vec!["mvn com.company:style-plugin:check".into()],
        depends_on: vec!["build".into()],
        ..Default::default()
    }
}
```

**2. Wire it in** `src/strategy/maven/mod.rs`:

```rust
mod compliance;  // add this line

// In steps(), add:
steps.push(compliance::step(info));
```

No changes to base, other languages, or the trait.

## How to Add a New Language

1. Create `src/strategy/<lang>/mod.rs` implementing `PipelineStrategy`
2. Create step files for language-specific steps
3. Add corresponding detector in `src/detector/` (if not already present)
4. Register in `strategy_for()` match

## Migration Plan

- The current `generate_pipeline()` function in `src/detector/mod.rs` is replaced by `strategy::generate_pipeline()`
- `detect_and_generate()` calls `strategy::generate_pipeline(info)` instead of the local `generate_pipeline(&info)`
- The `ProjectInfo` struct stays in detector module, unchanged
- Existing detector tests remain valid; new strategy tests cover step generation

## Testing Strategy

- **Unit tests per step file**: verify each step function returns correct StepDef fields
- **Unit tests per strategy**: verify steps() returns the right set of steps with correct ordering and dependencies
- **Integration test**: detector + strategy together produces valid pipeline.yml for sample projects
- **Base override test**: verify a language strategy can override base step defaults (e.g., on_failure)
