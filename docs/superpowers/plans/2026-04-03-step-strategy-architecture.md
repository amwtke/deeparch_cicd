# Step Strategy 架构实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 strategy 模块，将 pipeline step 生成逻辑从 detector 中解耦，每个语言策略继承 base 标准 step 并可覆盖/扩展。

**Architecture:** BaseStrategy 提供 build/test/lint/fmt 四个标准 step 工厂方法。6 个语言策略各自实现 PipelineStrategy trait，组合 base step 和语言特有 step。detector 模块中的 `generate_pipeline()` 迁移到 strategy 模块。

**Tech Stack:** Rust, 现有 pipeline/detector 模块

---

## 文件结构

| 操作 | 文件路径 | 职责 |
|------|---------|------|
| 创建 | `src/strategy/mod.rs` | PipelineStrategy trait, StepDef struct, Default impl, strategy_for() 注册表, generate_pipeline() |
| 创建 | `src/strategy/base/mod.rs` | BaseStrategy: build_step, test_step, lint_step, fmt_step |
| 创建 | `src/strategy/maven/mod.rs` | MavenStrategy impl |
| 创建 | `src/strategy/maven/checkstyle.rs` | Maven checkstyle step |
| 创建 | `src/strategy/maven/package.rs` | Maven package step |
| 创建 | `src/strategy/gradle/mod.rs` | GradleStrategy impl |
| 创建 | `src/strategy/gradle/checkstyle.rs` | Gradle checkstyle step |
| 创建 | `src/strategy/rust_lang/mod.rs` | RustStrategy impl |
| 创建 | `src/strategy/rust_lang/clippy.rs` | Rust clippy step |
| 创建 | `src/strategy/node/mod.rs` | NodeStrategy impl |
| 创建 | `src/strategy/node/typecheck.rs` | TypeScript typecheck step |
| 创建 | `src/strategy/python/mod.rs` | PythonStrategy impl |
| 创建 | `src/strategy/python/mypy.rs` | Python mypy step |
| 创建 | `src/strategy/go/mod.rs` | GoStrategy impl |
| 创建 | `src/strategy/go/vet.rs` | Go vet step |
| 修改 | `src/main.rs` | 添加 `mod strategy;` |
| 修改 | `src/detector/mod.rs` | `detect_and_generate()` 改用 `strategy::generate_pipeline()`，删除旧 `generate_pipeline()` |

---

### Task 1: StepDef 结构体 + PipelineStrategy trait + 注册表骨架

**Files:**
- 创建: `src/strategy/mod.rs`
- 修改: `src/main.rs`

- [ ] **Step 1: 创建 strategy/mod.rs，定义核心类型**

```rust
// src/strategy/mod.rs
pub mod base;

use std::collections::HashMap;

use crate::detector::{ProjectInfo, ProjectType};
use crate::pipeline::{OnFailure, Pipeline, Step};

/// Definition of a single pipeline step, produced by strategies.
#[derive(Debug, Clone)]
pub struct StepDef {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
    pub on_failure: Option<OnFailure>,
    pub allow_failure: bool,
}

impl Default for StepDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            commands: vec![],
            depends_on: vec![],
            workdir: "/workspace".into(),
            on_failure: None,
            allow_failure: false,
        }
    }
}

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

/// All language strategies implement this trait.
pub trait PipelineStrategy {
    /// Return all steps for this project (base + language-specific).
    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef>;

    /// Pipeline name (e.g., "maven-java-ci").
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
}

/// Map ProjectType to the corresponding strategy.
fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        // 暂时全部用 base fallback，后续 task 逐个替换
        _ => Box::new(base::BaseOnlyStrategy),
    }
}

/// Generate a Pipeline from ProjectInfo using the strategy system.
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

- [ ] **Step 2: 在 main.rs 注册 strategy 模块**

在 `src/main.rs` 的 `mod scheduler;` 后面添加一行：

```rust
mod strategy;
```

- [ ] **Step 3: 编译验证**

运行: `cargo build 2>&1 | tail -5`
预期: 编译失败，因为 `base::BaseOnlyStrategy` 还不存在——这是预期的，下一个 task 处理。

- [ ] **Step 4: 提交**

```bash
git add src/strategy/mod.rs src/main.rs
git commit -m "feat: add strategy module skeleton with StepDef, PipelineStrategy trait"
```

---

### Task 2: BaseStrategy 实现

**Files:**
- 创建: `src/strategy/base/mod.rs`

- [ ] **Step 1: 创建 base/mod.rs，写测试**

```rust
// src/strategy/base/mod.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::{PipelineStrategy, StepDef};

/// Provides factory methods for the four standard pipeline steps.
pub struct BaseStrategy;

impl BaseStrategy {
    /// Standard build step. AutoFix with 3 retries.
    pub fn build_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "build".into(),
            image: info.image.clone(),
            commands: info.build_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 3,
                context_paths: [&info.source_paths[..], &info.config_files[..]].concat(),
            }),
            ..Default::default()
        }
    }

    /// Standard test step. Depends on "build". Notify on failure.
    pub fn test_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "test".into(),
            image: info.image.clone(),
            commands: info.test_cmd.clone(),
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::Notify,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    /// Standard lint step. Returns None if info.lint_cmd is None. AutoFix with 2 retries.
    pub fn lint_step(info: &ProjectInfo) -> Option<StepDef> {
        info.lint_cmd.as_ref().map(|cmd| StepDef {
            name: "lint".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }

    /// Standard fmt-check step. Returns None if info.fmt_cmd is None. AutoFix with 1 retry.
    pub fn fmt_step(info: &ProjectInfo) -> Option<StepDef> {
        info.fmt_cmd.as_ref().map(|cmd| StepDef {
            name: "fmt-check".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 1,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }
}

/// Fallback strategy that uses only base steps. Used as default before
/// language-specific strategies are registered.
pub struct BaseOnlyStrategy;

impl PipelineStrategy for BaseOnlyStrategy {
    fn pipeline_name(&self, info: &ProjectInfo) -> String {
        format!(
            "{}-ci",
            format!("{}", info.project_type)
                .to_lowercase()
                .replace('/', "-")
        )
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }

        steps.push(BaseStrategy::test_step(info));

        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn sample_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_build_step_defaults() {
        let info = sample_info();
        let step = BaseStrategy::build_step(&info);
        assert_eq!(step.name, "build");
        assert_eq!(step.image, "rust:1.78-slim");
        assert_eq!(step.commands, vec!["cargo build"]);
        assert!(step.depends_on.is_empty());
        assert_eq!(step.workdir, "/workspace");
        let of = step.on_failure.unwrap();
        assert_eq!(of.strategy, Strategy::AutoFix);
        assert_eq!(of.max_retries, 3);
        assert_eq!(of.context_paths, vec!["src/", "Cargo.toml"]);
    }

    #[test]
    fn test_test_step_depends_on_build() {
        let info = sample_info();
        let step = BaseStrategy::test_step(&info);
        assert_eq!(step.name, "test");
        assert_eq!(step.depends_on, vec!["build"]);
        let of = step.on_failure.unwrap();
        assert_eq!(of.strategy, Strategy::Notify);
        assert_eq!(of.max_retries, 0);
    }

    #[test]
    fn test_lint_step_returns_none_when_no_lint_cmd() {
        let mut info = sample_info();
        info.lint_cmd = None;
        assert!(BaseStrategy::lint_step(&info).is_none());
    }

    #[test]
    fn test_lint_step_returns_some() {
        let info = sample_info();
        let step = BaseStrategy::lint_step(&info).unwrap();
        assert_eq!(step.name, "lint");
        let of = step.on_failure.unwrap();
        assert_eq!(of.max_retries, 2);
    }

    #[test]
    fn test_fmt_step_returns_none_when_no_fmt_cmd() {
        let mut info = sample_info();
        info.fmt_cmd = None;
        assert!(BaseStrategy::fmt_step(&info).is_none());
    }

    #[test]
    fn test_base_only_strategy_full_steps() {
        let info = sample_info();
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "lint");
        assert_eq!(steps[2].name, "test");
        assert_eq!(steps[3].name, "fmt-check");
    }

    #[test]
    fn test_base_only_strategy_minimal_steps() {
        let mut info = sample_info();
        info.lint_cmd = None;
        info.fmt_cmd = None;
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
    }

    #[test]
    fn test_base_only_pipeline_name() {
        let info = sample_info();
        let strategy = BaseOnlyStrategy;
        assert_eq!(strategy.pipeline_name(&info), "rust-ci");
    }
}
```

- [ ] **Step 2: 运行测试验证**

运行: `cargo test strategy 2>&1 | tail -20`
预期: 所有 base 测试通过

- [ ] **Step 3: 提交**

```bash
git add src/strategy/base/mod.rs
git commit -m "feat: implement BaseStrategy with build/test/lint/fmt step factories"
```

---

### Task 3: MavenStrategy + 特有 steps

**Files:**
- 创建: `src/strategy/maven/mod.rs`
- 创建: `src/strategy/maven/checkstyle.rs`
- 创建: `src/strategy/maven/package.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 maven/checkstyle.rs**

```rust
// src/strategy/maven/checkstyle.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "checkstyle".into(),
        image: info.image.clone(),
        commands: vec!["mvn checkstyle:check".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.config_files.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 maven/package.rs**

```rust
// src/strategy/maven/package.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "package".into(),
        image: info.image.clone(),
        commands: vec!["mvn package -DskipTests".into()],
        depends_on: vec!["test".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::Abort,
            max_retries: 0,
            context_paths: vec![],
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 3: 创建 maven/mod.rs**

```rust
// src/strategy/maven/mod.rs
mod checkstyle;
mod package;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct MavenStrategy;

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![
            BaseStrategy::build_step(info),
        ];

        // Add checkstyle if lint_cmd is present (detector found maven-checkstyle-plugin)
        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
        }

        steps.push(BaseStrategy::test_step(info));
        steps.push(package::step(info));

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn maven_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: Some("spring-boot 3.2.0".into()),
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: Some(vec!["mvn checkstyle:check".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into(), "src/main/resources/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_maven_steps_with_checkstyle() {
        let info = maven_info();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "checkstyle", "test", "package"]);
    }

    #[test]
    fn test_maven_steps_without_checkstyle() {
        let mut info = maven_info();
        info.lint_cmd = None;
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "test", "package"]);
    }

    #[test]
    fn test_maven_pipeline_name() {
        let info = maven_info();
        assert_eq!(MavenStrategy.pipeline_name(&info), "maven-java-ci");
    }

    #[test]
    fn test_package_depends_on_test() {
        let info = maven_info();
        let steps = MavenStrategy.steps(&info);
        let pkg = steps.iter().find(|s| s.name == "package").unwrap();
        assert_eq!(pkg.depends_on, vec!["test"]);
    }

    #[test]
    fn test_checkstyle_depends_on_build() {
        let info = maven_info();
        let steps = MavenStrategy.steps(&info);
        let cs = steps.iter().find(|s| s.name == "checkstyle").unwrap();
        assert_eq!(cs.depends_on, vec!["build"]);
    }
}
```

- [ ] **Step 4: 在 strategy/mod.rs 注册 maven 模块，更新 strategy_for()**

在 `src/strategy/mod.rs` 顶部添加 `pub mod maven;`，并在 `strategy_for()` 中添加：

```rust
pub mod base;
pub mod maven;

// ...

fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        ProjectType::Maven => Box::new(maven::MavenStrategy),
        _ => Box::new(base::BaseOnlyStrategy),
    }
}
```

- [ ] **Step 5: 运行测试**

运行: `cargo test strategy::maven 2>&1 | tail -15`
预期: 所有 maven 测试通过

- [ ] **Step 6: 提交**

```bash
git add src/strategy/maven/
git commit -m "feat: implement MavenStrategy with checkstyle and package steps"
```

---

### Task 4: GradleStrategy + 特有 steps

**Files:**
- 创建: `src/strategy/gradle/mod.rs`
- 创建: `src/strategy/gradle/checkstyle.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 gradle/checkstyle.rs**

```rust
// src/strategy/gradle/checkstyle.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "checkstyle".into(),
        image: info.image.clone(),
        commands: vec!["./gradlew check -x test".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.config_files.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 gradle/mod.rs**

```rust
// src/strategy/gradle/mod.rs
mod checkstyle;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct GradleStrategy;

impl PipelineStrategy for GradleStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "gradle-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
        }

        steps.push(BaseStrategy::test_step(info));

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn gradle_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: Some("spring-boot 3.2.0".into()),
            image: "gradle:8-jdk17".into(),
            build_cmd: vec!["./gradlew build -x test".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: Some(vec!["./gradlew check -x test".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_gradle_steps_with_checkstyle() {
        let info = gradle_info();
        let steps = GradleStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "checkstyle", "test"]);
    }

    #[test]
    fn test_gradle_steps_without_lint() {
        let mut info = gradle_info();
        info.lint_cmd = None;
        let steps = GradleStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "test"]);
    }

    #[test]
    fn test_gradle_pipeline_name() {
        let info = gradle_info();
        assert_eq!(GradleStrategy.pipeline_name(&info), "gradle-java-ci");
    }
}
```

- [ ] **Step 3: 在 strategy/mod.rs 注册 gradle**

```rust
pub mod gradle;

// strategy_for() 中添加：
ProjectType::Gradle => Box::new(gradle::GradleStrategy),
```

- [ ] **Step 4: 运行测试**

运行: `cargo test strategy::gradle 2>&1 | tail -10`
预期: 通过

- [ ] **Step 5: 提交**

```bash
git add src/strategy/gradle/
git commit -m "feat: implement GradleStrategy with checkstyle step"
```

---

### Task 5: RustStrategy + clippy step

**Files:**
- 创建: `src/strategy/rust_lang/mod.rs`
- 创建: `src/strategy/rust_lang/clippy.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 rust_lang/clippy.rs**

```rust
// src/strategy/rust_lang/clippy.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "clippy".into(),
        image: info.image.clone(),
        commands: vec!["cargo clippy -- -D warnings".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 rust_lang/mod.rs**

```rust
// src/strategy/rust_lang/mod.rs
mod clippy;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct RustStrategy;

impl PipelineStrategy for RustStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "rust-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        steps.push(clippy::step(info));

        steps.push(BaseStrategy::test_step(info));

        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn rust_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_rust_steps() {
        let info = rust_info();
        let steps = RustStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "clippy", "test", "fmt-check"]);
    }

    #[test]
    fn test_rust_clippy_always_present() {
        let mut info = rust_info();
        info.lint_cmd = None; // clippy step is added directly, not from lint_cmd
        let steps = RustStrategy.steps(&info);
        assert!(steps.iter().any(|s| s.name == "clippy"));
    }

    #[test]
    fn test_rust_pipeline_name() {
        let info = rust_info();
        assert_eq!(RustStrategy.pipeline_name(&info), "rust-ci");
    }
}
```

- [ ] **Step 3: 在 strategy/mod.rs 注册 rust_lang**

```rust
pub mod rust_lang;

// strategy_for() 中添加：
ProjectType::Rust => Box::new(rust_lang::RustStrategy),
```

- [ ] **Step 4: 运行测试**

运行: `cargo test strategy::rust_lang 2>&1 | tail -10`
预期: 通过

- [ ] **Step 5: 提交**

```bash
git add src/strategy/rust_lang/
git commit -m "feat: implement RustStrategy with clippy step"
```

---

### Task 6: NodeStrategy + typecheck step

**Files:**
- 创建: `src/strategy/node/mod.rs`
- 创建: `src/strategy/node/typecheck.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 node/typecheck.rs**

```rust
// src/strategy/node/typecheck.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

/// TypeScript type-check step. Only relevant for projects with TypeScript.
pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "typecheck".into(),
        image: info.image.clone(),
        commands: vec!["npx tsc --noEmit".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 node/mod.rs**

```rust
// src/strategy/node/mod.rs
mod typecheck;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct NodeStrategy;

impl NodeStrategy {
    /// Check if this is a TypeScript project by looking at framework info
    /// or config files for tsconfig.json hints.
    fn is_typescript(info: &ProjectInfo) -> bool {
        info.config_files.iter().any(|f| f.contains("tsconfig"))
            || info.framework.as_ref().map_or(false, |f| f.contains("next") || f.contains("angular"))
    }
}

impl PipelineStrategy for NodeStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "node-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        if Self::is_typescript(info) {
            steps.push(typecheck::step(info));
        }

        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }

        steps.push(BaseStrategy::test_step(info));

        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn node_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some("20".into()),
            framework: Some("next".into()),
            image: "node:20-slim".into(),
            build_cmd: vec!["npm install && npm run build".into()],
            test_cmd: vec!["npm test".into()],
            lint_cmd: Some(vec!["npm run lint".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["package.json".into(), "tsconfig.json".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_node_typescript_steps() {
        let info = node_info();
        let steps = NodeStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "typecheck", "lint", "test"]);
    }

    #[test]
    fn test_node_no_typescript() {
        let mut info = node_info();
        info.framework = Some("express".into());
        info.config_files = vec!["package.json".into()];
        let steps = NodeStrategy.steps(&info);
        assert!(!steps.iter().any(|s| s.name == "typecheck"));
    }

    #[test]
    fn test_node_pipeline_name() {
        let info = node_info();
        assert_eq!(NodeStrategy.pipeline_name(&info), "node-ci");
    }
}
```

- [ ] **Step 3: 在 strategy/mod.rs 注册 node**

```rust
pub mod node;

// strategy_for() 中添加：
ProjectType::Node => Box::new(node::NodeStrategy),
```

- [ ] **Step 4: 运行测试**

运行: `cargo test strategy::node 2>&1 | tail -10`
预期: 通过

- [ ] **Step 5: 提交**

```bash
git add src/strategy/node/
git commit -m "feat: implement NodeStrategy with typecheck step"
```

---

### Task 7: PythonStrategy + mypy step

**Files:**
- 创建: `src/strategy/python/mod.rs`
- 创建: `src/strategy/python/mypy.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 python/mypy.rs**

```rust
// src/strategy/python/mypy.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "mypy".into(),
        image: info.image.clone(),
        commands: vec!["pip install mypy && mypy .".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 python/mod.rs**

```rust
// src/strategy/python/mod.rs
mod mypy;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct PythonStrategy;

impl PipelineStrategy for PythonStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "python-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }

        steps.push(mypy::step(info));
        steps.push(BaseStrategy::test_step(info));

        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn python_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Python,
            language_version: Some("3.12".into()),
            framework: None,
            image: "python:3.12-slim".into(),
            build_cmd: vec!["pip install -e .".into()],
            test_cmd: vec!["pytest".into()],
            lint_cmd: Some(vec!["ruff check .".into()]),
            fmt_cmd: Some(vec!["ruff format --check .".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["pyproject.toml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_python_steps() {
        let info = python_info();
        let steps = PythonStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "lint", "mypy", "test", "fmt-check"]);
    }

    #[test]
    fn test_python_pipeline_name() {
        let info = python_info();
        assert_eq!(PythonStrategy.pipeline_name(&info), "python-ci");
    }
}
```

- [ ] **Step 3: 在 strategy/mod.rs 注册 python**

```rust
pub mod python;

// strategy_for() 中添加：
ProjectType::Python => Box::new(python::PythonStrategy),
```

- [ ] **Step 4: 运行测试**

运行: `cargo test strategy::python 2>&1 | tail -10`
预期: 通过

- [ ] **Step 5: 提交**

```bash
git add src/strategy/python/
git commit -m "feat: implement PythonStrategy with mypy step"
```

---

### Task 8: GoStrategy + vet step

**Files:**
- 创建: `src/strategy/go/mod.rs`
- 创建: `src/strategy/go/vet.rs`
- 修改: `src/strategy/mod.rs`

- [ ] **Step 1: 创建 go/vet.rs**

```rust
// src/strategy/go/vet.rs
use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "vet".into(),
        image: info.image.clone(),
        commands: vec!["go vet ./...".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 创建 go/mod.rs**

```rust
// src/strategy/go/mod.rs
mod vet;

use crate::detector::ProjectInfo;
use crate::strategy::base::BaseStrategy;
use crate::strategy::{PipelineStrategy, StepDef};

pub struct GoStrategy;

impl PipelineStrategy for GoStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "go-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];

        steps.push(vet::step(info));

        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }

        steps.push(BaseStrategy::test_step(info));

        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }

        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::ProjectType;

    fn go_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: Some(vec!["golangci-lint run".into()]),
            fmt_cmd: Some(vec!["gofmt -l . | grep -q . && exit 1 || true".into()]),
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_go_steps() {
        let info = go_info();
        let steps = GoStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "vet", "lint", "test", "fmt-check"]);
    }

    #[test]
    fn test_go_steps_no_lint() {
        let mut info = go_info();
        info.lint_cmd = None;
        info.fmt_cmd = None;
        let steps = GoStrategy.steps(&info);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["build", "vet", "test"]);
    }

    #[test]
    fn test_go_pipeline_name() {
        let info = go_info();
        assert_eq!(GoStrategy.pipeline_name(&info), "go-ci");
    }
}
```

- [ ] **Step 3: 在 strategy/mod.rs 注册 go，完成所有语言注册**

此时 `strategy/mod.rs` 的模块声明和 `strategy_for()` 应该完整：

```rust
pub mod base;
pub mod maven;
pub mod gradle;
pub mod rust_lang;
pub mod node;
pub mod python;
pub mod go;

// ...

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
```

- [ ] **Step 4: 运行全部 strategy 测试**

运行: `cargo test strategy 2>&1 | tail -30`
预期: 所有 strategy 测试通过

- [ ] **Step 5: 提交**

```bash
git add src/strategy/go/ src/strategy/mod.rs
git commit -m "feat: implement GoStrategy with vet step, complete all language strategies"
```

---

### Task 9: 迁移 — detector 改用 strategy::generate_pipeline()

**Files:**
- 修改: `src/detector/mod.rs`

- [ ] **Step 1: 修改 detect_and_generate() 调用 strategy 模块**

在 `src/detector/mod.rs` 中，修改 `detect_and_generate()`:

```rust
/// Auto-detect project type and generate pipeline
pub fn detect_and_generate(dir: &Path) -> Result<(ProjectInfo, Pipeline)> {
    let detectors = all_detectors();

    for detector in &detectors {
        if detector.detect(dir) {
            let info = detector.analyze(dir)?;
            let pipeline = crate::strategy::generate_pipeline(&info);
            return Ok((info, pipeline));
        }
    }

    anyhow::bail!(
        "Could not detect project type in '{}'. Supported: Maven, Gradle, Rust, Node.js, Python, Go",
        dir.display()
    );
}
```

- [ ] **Step 2: 删除旧的 generate_pipeline() 函数和它的 use 语句**

从 `src/detector/mod.rs` 中删除：
- `use crate::pipeline::{Pipeline, Step, OnFailure, Strategy};` 行中不再需要的 `Step, OnFailure, Strategy`（保留 `Pipeline`）
- 整个 `fn generate_pipeline(info: &ProjectInfo) -> Pipeline { ... }` 函数（约第 103-183 行）

保留的 use 行：
```rust
use crate::pipeline::Pipeline;
```

- [ ] **Step 3: 更新 detector/mod.rs 中的测试**

旧测试直接调用 `generate_pipeline()`，改为调用 `crate::strategy::generate_pipeline()`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pipeline_basic() {
        let info = ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
        };
        let pipeline = crate::strategy::generate_pipeline(&info);
        assert_eq!(pipeline.name, "rust-ci");
        // RustStrategy: build, clippy, test, fmt-check
        assert_eq!(pipeline.steps.len(), 4);
        assert_eq!(pipeline.steps[0].name, "build");
    }

    #[test]
    fn test_generate_pipeline_no_lint_or_fmt() {
        let info = ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
        };
        let pipeline = crate::strategy::generate_pipeline(&info);
        // GoStrategy: build, vet, test (no lint, no fmt)
        assert_eq!(pipeline.steps.len(), 3);
        assert_eq!(pipeline.steps[0].name, "build");
        assert_eq!(pipeline.steps[1].name, "vet");
        assert_eq!(pipeline.steps[2].name, "test");
    }
}
```

- [ ] **Step 4: 运行全部测试**

运行: `cargo test 2>&1 | tail -30`
预期: 所有测试通过（detector 测试 + strategy 测试 + 集成测试）

- [ ] **Step 5: 提交**

```bash
git add src/detector/mod.rs
git commit -m "refactor: migrate pipeline generation from detector to strategy module"
```

---

### Task 10: 端到端验证 + 推送

**Files:** 无新文件

- [ ] **Step 1: 编译验证**

运行: `cargo build 2>&1 | tail -5`
预期: 编译成功

- [ ] **Step 2: 运行全部测试**

运行: `cargo test 2>&1 | tail -40`
预期: 所有测试通过

- [ ] **Step 3: 在真实项目上测试 init**

运行:
```bash
cd /Users/xiaojin/workspace/capinfo/wyproject-master/src
/Users/xiaojin/workspace/deeparch_cicd/target/debug/pipelight init
cat pipeline.yml
```
预期: 生成的 pipeline.yml 包含 build, checkstyle（如有）, test, package 四个 step

- [ ] **Step 4: 推送全部提交**

运行: `git push`
