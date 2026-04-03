# 实时输出增强实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 增强 pipelight CLI 的输出能力，实现实时日志流、多行进度条、UT 结果解析、step 耗时统计表，以及 `--verbose` 控制。

**Architecture:** Executor 层增加 `on_log` 回调实现实时日志推送；Output 层重写 TTY 模式（基于 indicatif MultiProgress）和 Plain 模式；Strategy 层扩展 `parse_test_output()` 方法解析各语言测试结果；RunState 层新增 `test_summary` 字段。

**Tech Stack:** Rust, indicatif (MultiProgress/ProgressBar), console (styled output), regex (UT 解析), serde (序列化)

---

## Task 1: TestSummary 结构体 + parse_test_output() trait 扩展

**目标:** 创建 `TestSummary` 数据结构和 trait 默认方法，为后续各语言解析器提供基础。

**文件变更:**
- 新建 `src/strategy/test_parser.rs`
- 修改 `src/strategy/mod.rs`

### 步骤

- [ ] 1.1 创建 `src/strategy/test_parser.rs`，定义 `TestSummary` 结构体

```rust
// src/strategy/test_parser.rs

use serde::{Deserialize, Serialize};

/// Aggregated test result summary parsed from step output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestSummary {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

impl TestSummary {
    pub fn new(passed: u32, failed: u32, skipped: u32) -> Self {
        Self { passed, failed, skipped }
    }

    /// Total number of tests.
    pub fn total(&self) -> u32 {
        self.passed + self.failed + self.skipped
    }

    /// Whether all tests passed (none failed).
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_new() {
        let s = TestSummary::new(10, 2, 3);
        assert_eq!(s.passed, 10);
        assert_eq!(s.failed, 2);
        assert_eq!(s.skipped, 3);
    }

    #[test]
    fn test_summary_total() {
        let s = TestSummary::new(10, 2, 3);
        assert_eq!(s.total(), 15);
    }

    #[test]
    fn test_summary_all_passed() {
        assert!(TestSummary::new(10, 0, 2).all_passed());
        assert!(!TestSummary::new(10, 1, 0).all_passed());
    }

    #[test]
    fn test_summary_serialize_json() {
        let s = TestSummary::new(42, 0, 2);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"passed\":42"));
        assert!(json.contains("\"failed\":0"));
        assert!(json.contains("\"skipped\":2"));
    }

    #[test]
    fn test_summary_deserialize_json() {
        let json = r#"{"passed":5,"failed":1,"skipped":0}"#;
        let s: TestSummary = serde_json::from_str(json).unwrap();
        assert_eq!(s, TestSummary::new(5, 1, 0));
    }

    #[test]
    fn test_summary_zero() {
        let s = TestSummary::new(0, 0, 0);
        assert_eq!(s.total(), 0);
        assert!(s.all_passed());
    }
}
```

- [ ] 1.2 修改 `src/strategy/mod.rs`：添加 `pub mod test_parser;` 并扩展 `PipelineStrategy` trait

在 `src/strategy/mod.rs` 的模块声明区域最上方，添加：

```rust
pub mod test_parser;
```

在文件顶部 use 区域，添加：

```rust
use crate::strategy::test_parser::TestSummary;
```

在 `PipelineStrategy` trait 定义中，在现有两个方法之后添加：

```rust
    /// Parse test output to extract test summary (passed/failed/skipped counts).
    /// Default returns None; each language strategy overrides with language-specific parsing.
    fn parse_test_output(&self, _output: &str) -> Option<TestSummary> {
        None
    }
```

- [ ] 1.3 验证编译通过：`cargo build`
- [ ] 1.4 运行测试：`cargo test strategy::test_parser`
- [ ] 1.5 提交：`git add src/strategy/test_parser.rs src/strategy/mod.rs && git commit -m "feat: add TestSummary struct and parse_test_output() trait method"`

---

## Task 2: 6 种语言的 UT 解析器实现

**目标:** 为 Maven、Gradle、Rust、Node、Python、Go 6 种策略分别实现 `parse_test_output()`，每个含单元测试。

**文件变更:**
- 修改 `src/strategy/maven/mod.rs`
- 修改 `src/strategy/gradle/mod.rs`
- 修改 `src/strategy/rust_lang/mod.rs`
- 修改 `src/strategy/node/mod.rs`
- 修改 `src/strategy/python/mod.rs`
- 修改 `src/strategy/go/mod.rs`

### 步骤

- [ ] 2.1 Maven 解析器 — 修改 `src/strategy/maven/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for MavenStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        // Maven outputs one "Tests run:" line per module in multi-module builds.
        // We must sum all matches.
        let re = Regex::new(r"Tests run: (\d+), Failures: (\d+), Errors: (\d+), Skipped: (\d+)")
            .unwrap();
        let mut total_run = 0u32;
        let mut total_fail = 0u32;
        let mut total_err = 0u32;
        let mut total_skip = 0u32;
        let mut found = false;

        for cap in re.captures_iter(output) {
            found = true;
            total_run += cap[1].parse::<u32>().unwrap_or(0);
            total_fail += cap[2].parse::<u32>().unwrap_or(0);
            total_err += cap[3].parse::<u32>().unwrap_or(0);
            total_skip += cap[4].parse::<u32>().unwrap_or(0);
        }

        if found {
            let failed = total_fail + total_err;
            let passed = total_run.saturating_sub(failed + total_skip);
            Some(TestSummary::new(passed, failed, total_skip))
        } else {
            None
        }
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_test_output_single_module() {
        let strategy = MavenStrategy;
        let output = r#"
[INFO] -------------------------------------------------------
[INFO]  T E S T S
[INFO] -------------------------------------------------------
[INFO] Running com.example.AppTest
[INFO] Tests run: 42, Failures: 0, Errors: 0, Skipped: 2
[INFO] -------------------------------------------------------
[INFO] BUILD SUCCESS
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 40);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_parse_test_output_multi_module() {
        let strategy = MavenStrategy;
        let output = r#"
[INFO] Tests run: 10, Failures: 0, Errors: 0, Skipped: 1
[INFO] Tests run: 20, Failures: 1, Errors: 0, Skipped: 0
[INFO] Tests run: 12, Failures: 0, Errors: 2, Skipped: 0
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        // total_run=42, fail=1, err=2, skip=1 => passed=42-3-1=38
        assert_eq!(summary.passed, 38);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_parse_test_output_no_tests() {
        let strategy = MavenStrategy;
        let output = "[INFO] BUILD SUCCESS\n";
        assert!(strategy.parse_test_output(output).is_none());
    }
```

- [ ] 2.2 Gradle 解析器 — 修改 `src/strategy/gradle/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for GradleStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let re = Regex::new(r"(\d+) tests completed, (\d+) failed").unwrap();
        if let Some(cap) = re.captures(output) {
            let completed: u32 = cap[1].parse().unwrap_or(0);
            let failed: u32 = cap[2].parse().unwrap_or(0);
            let passed = completed.saturating_sub(failed);
            // Gradle skipped detection
            let skip_re = Regex::new(r"(\d+) skipped").unwrap();
            let skipped = skip_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            Some(TestSummary::new(passed, failed, skipped))
        } else {
            None
        }
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_test_output_gradle() {
        let strategy = GradleStrategy;
        let output = "42 tests completed, 0 failed\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 42);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_test_output_gradle_with_failures() {
        let strategy = GradleStrategy;
        let output = "50 tests completed, 3 failed\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 47);
        assert_eq!(summary.failed, 3);
    }

    #[test]
    fn test_parse_test_output_gradle_with_skipped() {
        let strategy = GradleStrategy;
        let output = "42 tests completed, 0 failed, 5 skipped\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 42);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 5);
    }

    #[test]
    fn test_parse_test_output_gradle_no_match() {
        let strategy = GradleStrategy;
        let output = "BUILD SUCCESSFUL\n";
        assert!(strategy.parse_test_output(output).is_none());
    }
```

- [ ] 2.3 Rust 解析器 — 修改 `src/strategy/rust_lang/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for RustStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        let re = Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored")
            .unwrap();
        // Rust can have multiple test result lines (lib tests, integration tests, etc.)
        let mut total_passed = 0u32;
        let mut total_failed = 0u32;
        let mut total_ignored = 0u32;
        let mut found = false;

        for cap in re.captures_iter(output) {
            found = true;
            total_passed += cap[1].parse::<u32>().unwrap_or(0);
            total_failed += cap[2].parse::<u32>().unwrap_or(0);
            total_ignored += cap[3].parse::<u32>().unwrap_or(0);
        }

        if found {
            Some(TestSummary::new(total_passed, total_failed, total_ignored))
        } else {
            None
        }
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_test_output_rust() {
        let strategy = RustStrategy;
        let output = r#"
running 10 tests
test foo ... ok
test bar ... ok
test result: ok. 10 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 10);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_parse_test_output_rust_multiple_suites() {
        let strategy = RustStrategy;
        let output = r#"
test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured
test result: ok. 3 passed; 1 failed; 0 ignored; 0 measured
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 8);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_parse_test_output_rust_no_match() {
        let strategy = RustStrategy;
        assert!(strategy.parse_test_output("Compiling foo v0.1.0").is_none());
    }
```

- [ ] 2.4 Node 解析器 — 修改 `src/strategy/node/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for NodeStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        // Try Jest format first: "Tests:  2 failed, 8 passed, 10 total"
        let jest_re = Regex::new(r"Tests:\s+(?:(\d+) failed,\s+)?(\d+) passed").unwrap();
        if let Some(cap) = jest_re.captures(output) {
            let failed: u32 = cap.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let passed: u32 = cap[2].parse().unwrap_or(0);
            // Check for skipped/pending in Jest output
            let skip_re = Regex::new(r"(\d+) (?:skipped|pending|todo)").unwrap();
            let skipped = skip_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            return Some(TestSummary::new(passed, failed, skipped));
        }

        // Try Mocha format: "8 passing" / "2 failing"
        let mocha_pass_re = Regex::new(r"(\d+) passing").unwrap();
        if let Some(cap) = mocha_pass_re.captures(output) {
            let passed: u32 = cap[1].parse().unwrap_or(0);
            let fail_re = Regex::new(r"(\d+) failing").unwrap();
            let failed = fail_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            let pend_re = Regex::new(r"(\d+) pending").unwrap();
            let skipped = pend_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            return Some(TestSummary::new(passed, failed, skipped));
        }

        None
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_jest_output() {
        let strategy = NodeStrategy;
        let output = r#"
Test Suites: 1 passed, 1 total
Tests:       2 failed, 8 passed, 10 total
Snapshots:   0 total
Time:        3.456 s
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 8);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_jest_all_passing() {
        let strategy = NodeStrategy;
        let output = "Tests:       10 passed, 10 total\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 10);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn test_parse_mocha_output() {
        let strategy = NodeStrategy;
        let output = r#"
  8 passing (2s)
  2 failing
  1 pending
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 8);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_parse_mocha_passing_only() {
        let strategy = NodeStrategy;
        let output = "  15 passing (1s)\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 15);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_node_no_match() {
        let strategy = NodeStrategy;
        assert!(strategy.parse_test_output("npm run build: success").is_none());
    }
```

- [ ] 2.5 Python 解析器 — 修改 `src/strategy/python/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for PythonStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        // pytest format: "5 passed, 1 failed, 2 skipped" or "5 passed" etc.
        let re = Regex::new(r"(\d+) passed").unwrap();
        if let Some(cap) = re.captures(output) {
            let passed: u32 = cap[1].parse().unwrap_or(0);
            let fail_re = Regex::new(r"(\d+) failed").unwrap();
            let failed = fail_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            let skip_re = Regex::new(r"(\d+) skipped").unwrap();
            let skipped = skip_re
                .captures(output)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            return Some(TestSummary::new(passed, failed, skipped));
        }
        None
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_pytest_output() {
        let strategy = PythonStrategy;
        let output = "===== 5 passed, 1 failed, 2 skipped in 3.42s =====\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 5);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 2);
    }

    #[test]
    fn test_parse_pytest_all_passing() {
        let strategy = PythonStrategy;
        let output = "===== 20 passed in 1.23s =====\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 20);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_parse_pytest_no_match() {
        let strategy = PythonStrategy;
        assert!(strategy.parse_test_output("pip install OK").is_none());
    }
```

- [ ] 2.6 Go 解析器 — 修改 `src/strategy/go/mod.rs`

在文件顶部 use 区域添加：

```rust
use regex::Regex;
use crate::strategy::test_parser::TestSummary;
```

在 `impl PipelineStrategy for GoStrategy` 块中，在 `steps()` 方法之后添加：

```rust
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        // Go test output: each package produces either "ok  pkg" or "FAIL pkg" line.
        let ok_re = Regex::new(r"(?m)^ok\s+").unwrap();
        let fail_re = Regex::new(r"(?m)^FAIL\s+").unwrap();
        let skip_re = Regex::new(r"(?m)^\?\s+").unwrap(); // "? pkg [no test files]"

        let passed = ok_re.find_iter(output).count() as u32;
        let failed = fail_re.find_iter(output).count() as u32;
        let skipped = skip_re.find_iter(output).count() as u32;

        if passed + failed + skipped > 0 {
            Some(TestSummary::new(passed, failed, skipped))
        } else {
            None
        }
    }
```

在测试模块中添加：

```rust
    #[test]
    fn test_parse_go_test_output() {
        let strategy = GoStrategy;
        let output = r#"
ok  	github.com/example/pkg1	0.123s
ok  	github.com/example/pkg2	0.456s
FAIL	github.com/example/pkg3	0.789s
?   	github.com/example/pkg4	[no test files]
"#;
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_parse_go_test_all_pass() {
        let strategy = GoStrategy;
        let output = "ok  \tgithub.com/example/pkg\t0.5s\n";
        let summary = strategy.parse_test_output(output).unwrap();
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn test_parse_go_test_no_match() {
        let strategy = GoStrategy;
        assert!(strategy.parse_test_output("go build ./...").is_none());
    }
```

- [ ] 2.7 验证编译通过：`cargo build`
- [ ] 2.8 运行所有策略测试：`cargo test strategy::`
- [ ] 2.9 提交：`git add src/strategy/ && git commit -m "feat: implement UT parsers for all 6 language strategies"`

---

## Task 3: StepState + RunState 添加 test_summary 字段

**目标:** 在 `StepState` 中添加 `test_summary: Option<TestSummary>` 字段，确保 JSON 序列化/反序列化正确，并更新所有构造 `StepState` 的位置。

**文件变更:**
- 修改 `src/run_state/mod.rs`
- 修改 `src/cli/mod.rs`（所有 `StepState { ... }` 构造处）

### 步骤

- [ ] 3.1 修改 `src/run_state/mod.rs`

在文件顶部 use 区域添加：

```rust
use crate::strategy::test_parser::TestSummary;
```

在 `StepState` 结构体中，在 `on_failure` 字段之后添加：

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_summary: Option<TestSummary>,
```

- [ ] 3.2 更新 `src/run_state/mod.rs` 中所有测试里的 `StepState` 构造

在所有构造 `StepState { ... }` 的地方，添加 `test_summary: None,` 字段。涉及以下测试函数中的 `StepState` 构造：
- `test_update_step_status` (1 处)
- `test_retries_remaining` (1 处)
- `test_save_load_roundtrip_with_steps` (2 处)
- `test_decrement_retries_to_zero` (1 处)
- `test_decrement_retries_no_on_failure` (1 处)

每处在 `on_failure: ...` 行之后添加 `test_summary: None,`。

- [ ] 3.3 更新 `src/cli/mod.rs` 中所有 `StepState` 构造

在 `cmd_run` 函数中有两处 `StepState` 构造（一处正常执行，一处标记 Skipped），以及 `cmd_retry` 中无直接构造但有字段更新。每处 `StepState { ... }` 构造在 `on_failure` 字段之后添加 `test_summary: None,`。

`cmd_run` 第一处（约第 220 行）：

```rust
            state.add_step(StepState {
                name: result.step_name.clone(),
                status: step_status,
                exit_code: Some(result.exit_code),
                duration_ms: Some(result.duration.as_millis() as u64),
                image: pipeline_step.map(|s| s.image.clone()).unwrap_or_default(),
                command: pipeline_step.map(|s| s.commands.join(" && ")).unwrap_or_default(),
                stdout: if stdout.is_empty() { None } else { Some(stdout) },
                stderr: if stderr.is_empty() { None } else { Some(stderr) },
                error_context: None,
                on_failure: on_failure_state,
                test_summary: None, // <-- 新增
            });
```

`cmd_run` 第二处（约第 259 行，Skipped steps）：

```rust
                state.add_step(StepState {
                    name: step_name.clone(),
                    status: StepStatus::Skipped,
                    exit_code: None,
                    duration_ms: None,
                    image: pipeline.get_step(step_name).map(|s| s.image.clone()).unwrap_or_default(),
                    command: pipeline.get_step(step_name).map(|s| s.commands.join(" && ")).unwrap_or_default(),
                    stdout: None,
                    stderr: None,
                    error_context: None,
                    on_failure: None,
                    test_summary: None, // <-- 新增
                });
```

- [ ] 3.4 添加 JSON 序列化测试到 `src/run_state/mod.rs` 测试模块

```rust
    #[test]
    fn test_step_state_test_summary_serialization() {
        use crate::strategy::test_parser::TestSummary;

        let mut state = RunState::new("ts-1", "pipeline");
        state.add_step(StepState {
            name: "test".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: Some(5000),
            image: "rust:1.78".into(),
            command: "cargo test".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: Some(TestSummary::new(42, 0, 2)),
        });

        let json = serde_json::to_string_pretty(&state).unwrap();
        assert!(json.contains("\"test_summary\""));
        assert!(json.contains("\"passed\": 42"));
        assert!(json.contains("\"failed\": 0"));
        assert!(json.contains("\"skipped\": 2"));

        // Roundtrip
        let loaded: RunState = serde_json::from_str(&json).unwrap();
        let ts = loaded.get_step("test").unwrap().test_summary.as_ref().unwrap();
        assert_eq!(ts.passed, 42);
    }

    #[test]
    fn test_step_state_no_test_summary_omitted_in_json() {
        let step = StepState {
            name: "build".into(),
            status: StepStatus::Success,
            exit_code: Some(0),
            duration_ms: Some(1000),
            image: "alpine".into(),
            command: "echo hi".into(),
            stdout: None,
            stderr: None,
            error_context: None,
            on_failure: None,
            test_summary: None,
        };
        let json = serde_json::to_string(&step).unwrap();
        // skip_serializing_if should omit test_summary when None
        assert!(!json.contains("test_summary"));
    }
```

- [ ] 3.5 验证编译通过：`cargo build`
- [ ] 3.6 运行测试：`cargo test`
- [ ] 3.7 提交：`git add src/run_state/mod.rs src/cli/mod.rs && git commit -m "feat: add test_summary field to StepState with JSON serialization"`

---

## Task 4: Executor 实时流式回调

**目标:** 修改 `run_step()` 签名，添加 `on_log` 回调参数，在日志收集循环中调用。更新所有调用方。

**文件变更:**
- 修改 `src/executor/mod.rs`
- 修改 `src/cli/mod.rs`

### 步骤

- [ ] 4.1 修改 `src/executor/mod.rs` 中 `run_step` 签名

将：

```rust
    pub async fn run_step(&self, pipeline_name: &str, step: &Step, project_dir: &std::path::Path) -> Result<StepResult> {
```

改为：

```rust
    pub async fn run_step(
        &self,
        pipeline_name: &str,
        step: &Step,
        project_dir: &std::path::Path,
        on_log: impl Fn(&LogLine),
    ) -> Result<StepResult> {
```

- [ ] 4.2 在日志收集循环中调用 `on_log`

在 `run_step` 方法的日志收集循环中（约第 162 行），将：

```rust
                    logs.push(LogLine { stream, message });
```

改为：

```rust
                    let line = LogLine { stream, message };
                    on_log(&line);
                    logs.push(line);
```

- [ ] 4.3 更新 `src/cli/mod.rs` 中所有 `run_step` 调用

`cmd_run` 中 tokio::spawn 内的调用（约第 188 行），将：

```rust
                tokio::spawn(async move { executor.run_step(&pipeline_name, &step, &dir).await })
```

改为：

```rust
                tokio::spawn(async move { executor.run_step(&pipeline_name, &step, &dir, |_| {}).await })
```

`cmd_retry` 中第一处调用（约第 386 行），将：

```rust
    let result = executor.run_step(&pipeline.name, pipeline_step, &project_dir).await?;
```

改为：

```rust
    let result = executor.run_step(&pipeline.name, pipeline_step, &project_dir, |_| {}).await?;
```

`cmd_retry` 中第二处调用（约第 440 行，skipped step 重试），将：

```rust
            let sr = executor.run_step(&pipeline.name, skipped_step, &project_dir).await?;
```

改为：

```rust
            let sr = executor.run_step(&pipeline.name, skipped_step, &project_dir, |_| {}).await?;
```

- [ ] 4.4 验证编译通过：`cargo build`
- [ ] 4.5 运行测试：`cargo test`
- [ ] 4.6 提交：`git add src/executor/mod.rs src/cli/mod.rs && git commit -m "feat: add on_log streaming callback to executor run_step"`

---

## Task 5: --verbose 参数

**目标:** 在 Run 和 Retry 子命令上添加 `--verbose` flag，传递到命令处理函数中。

**文件变更:**
- 修改 `src/cli/mod.rs`

### 步骤

- [ ] 5.1 在 `Command::Run` 枚举变体中添加 verbose 参数

在 `run_id: Option<String>,` 之后添加：

```rust
        /// Enable verbose output (show all log lines in realtime)
        #[arg(long)]
        verbose: bool,
```

- [ ] 5.2 在 `Command::Retry` 枚举变体中添加 verbose 参数

在 `file: PathBuf,` 之后添加：

```rust
        /// Enable verbose output (show all log lines in realtime)
        #[arg(long)]
        verbose: bool,
```

- [ ] 5.3 更新 `dispatch()` 函数中的 `Command::Run` 匹配

将：

```rust
        Command::Run {
            file,
            step,
            dry_run,
            output,
            run_id,
        } => cmd_run(file, step, dry_run, output, run_id).await,
```

改为：

```rust
        Command::Run {
            file,
            step,
            dry_run,
            output,
            run_id,
            verbose,
        } => cmd_run(file, step, dry_run, output, run_id, verbose).await,
```

- [ ] 5.4 更新 `dispatch()` 函数中的 `Command::Retry` 匹配

将：

```rust
        Command::Retry {
            run_id,
            step,
            output,
            file,
        } => {
            let mode = resolve_output_mode(output);
            cmd_retry(run_id, step, mode, file).await
        }
```

改为：

```rust
        Command::Retry {
            run_id,
            step,
            output,
            file,
            verbose,
        } => {
            let mode = resolve_output_mode(output);
            cmd_retry(run_id, step, mode, file, verbose).await
        }
```

- [ ] 5.5 更新 `cmd_run` 函数签名

将：

```rust
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
) -> Result<i32> {
```

改为：

```rust
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
    verbose: bool,
) -> Result<i32> {
```

（`verbose` 参数暂时不使用，后续 Task 8 中接入 output 层。在函数开头添加 `let _verbose = verbose;` 抑制 unused 警告。）

- [ ] 5.6 更新 `cmd_retry` 函数签名

将：

```rust
async fn cmd_retry(
    run_id: String,
    step: Option<String>,
    mode: OutputMode,
    file: PathBuf,
) -> Result<i32> {
```

改为：

```rust
async fn cmd_retry(
    run_id: String,
    step: Option<String>,
    mode: OutputMode,
    file: PathBuf,
    verbose: bool,
) -> Result<i32> {
```

（同样暂时 `let _verbose = verbose;`。）

- [ ] 5.7 验证编译通过：`cargo build`
- [ ] 5.8 运行测试：`cargo test`
- [ ] 5.9 提交：`git add src/cli/mod.rs && git commit -m "feat: add --verbose flag to Run and Retry subcommands"`

---

## Task 6: TTY 进度条重写 (src/output/tty.rs)

**目标:** 新建 `PipelineProgressUI` 结构体，基于 `indicatif::MultiProgress` 实现多行进度条，支持实时日志显示和统计表输出。保留现有 `PipelineReporter` 中仍被使用的方法。

**文件变更:**
- 修改 `src/output/tty.rs`

### 步骤

- [ ] 6.1 在 `src/output/tty.rs` 文件顶部添加新的 imports

在现有 imports 之后添加：

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use crate::strategy::test_parser::TestSummary;
use crate::run_state::StepStatus;
```

- [ ] 6.2 在文件中（`PipelineReporter` 之后，`format_duration` 之前）添加 `PipelineProgressUI` 结构体和实现

```rust
/// Realtime progress UI for pipeline execution in TTY mode.
/// Uses indicatif MultiProgress to display all steps with status indicators.
pub struct PipelineProgressUI {
    mp: MultiProgress,
    bars: HashMap<String, ProgressBar>,
    log_bars: HashMap<String, Vec<ProgressBar>>,
    step_order: Vec<String>,
    start_times: HashMap<String, Instant>,
    verbose: bool,
    max_log_lines: usize,
}

impl PipelineProgressUI {
    /// Create a new progress UI with the given step names.
    pub fn new(step_names: &[String], verbose: bool) -> Self {
        let mp = MultiProgress::new();
        let mut bars = HashMap::new();
        let step_order: Vec<String> = step_names.to_vec();

        let pending_style = ProgressStyle::with_template("  {prefix} {wide_msg}")
            .unwrap();

        for name in &step_order {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(pending_style.clone());
            pb.set_prefix(format!("\u{2b1c}")); // ⬜
            pb.set_message(format!("{}      -", style(name).bold()));
            bars.insert(name.clone(), pb);
        }

        Self {
            mp,
            bars,
            log_bars: HashMap::new(),
            step_order,
            start_times: HashMap::new(),
            verbose,
            max_log_lines: if verbose { usize::MAX } else { 3 },
        }
    }

    /// Mark a step as started (switch to spinner).
    pub fn start_step(&mut self, name: &str) {
        self.start_times.insert(name.to_string(), Instant::now());
        if let Some(pb) = self.bars.get(name) {
            let spinner_style = ProgressStyle::with_template("  {spinner} {wide_msg}")
                .unwrap()
                .tick_strings(&["\u{23f3}", "\u{23f3}", "\u{23f3}", "\u{23f3}"]); // ⏳
            pb.set_style(spinner_style);
            pb.set_message(format!("{}  0.0s  Running...", style(name).bold()));
            pb.enable_steady_tick(Duration::from_millis(100));
        }
        // Initialize log lines storage
        self.log_bars.insert(name.to_string(), Vec::new());
    }

    /// Add a realtime log line under the current running step.
    pub fn log_line(&mut self, name: &str, message: &str) {
        let elapsed = self.start_times.get(name)
            .map(|s| s.elapsed())
            .unwrap_or_default();

        // Update step progress bar with elapsed time
        if let Some(pb) = self.bars.get(name) {
            pb.set_message(format!(
                "{}  {}  Running...",
                style(name).bold(),
                format_duration(elapsed)
            ));
        }

        // Add log line below the step bar
        let trimmed = message.trim_end();
        if trimmed.is_empty() {
            return;
        }

        if let Some(log_bars) = self.log_bars.get_mut(name) {
            if log_bars.len() >= self.max_log_lines && self.max_log_lines != usize::MAX {
                // Remove oldest log line
                if let Some(old) = log_bars.first() {
                    old.finish_and_clear();
                    // Can't easily remove from MultiProgress, so just clear it
                }
                log_bars.remove(0);
            }

            // Find position: after the step bar and existing log bars
            let step_idx = self.step_order.iter().position(|s| s == name);
            if let Some(_idx) = step_idx {
                let log_pb = if let Some(step_bar) = self.bars.get(name) {
                    self.mp.insert_after(step_bar, ProgressBar::new_spinner())
                } else {
                    self.mp.add(ProgressBar::new_spinner())
                };
                let log_style = ProgressStyle::with_template("    {wide_msg}").unwrap();
                log_pb.set_style(log_style);
                log_pb.set_message(format!(
                    "{} {}",
                    style("\u{2502}").dim(), // │
                    trimmed
                ));
                log_bars.push(log_pb);
            }
        }
    }

    /// Mark a step as finished (success or failure).
    pub fn finish_step(&mut self, name: &str, success: bool, duration: Duration) {
        // Clear log lines for this step
        if let Some(log_bars) = self.log_bars.remove(name) {
            for lb in log_bars {
                lb.finish_and_clear();
            }
        }

        if let Some(pb) = self.bars.get(name) {
            pb.disable_steady_tick();
            let finish_style = ProgressStyle::with_template("  {prefix} {wide_msg}")
                .unwrap();
            pb.set_style(finish_style);
            if success {
                pb.set_prefix(format!("{}", CHECK));
                pb.set_message(format!(
                    "{}  {}",
                    style(name).bold(),
                    format_duration(duration)
                ));
            } else {
                pb.set_prefix(format!("{}", CROSS));
                pb.set_message(format!(
                    "{}  {}",
                    style(name).red().bold(),
                    format_duration(duration)
                ));
            }
            pb.finish();
        }
    }

    /// Print test summary line.
    pub fn print_test_summary(&self, summary: &TestSummary) {
        self.mp.println(format!(
            "\n{} {}: {} passed, {} failed, {} skipped",
            Emoji("\u{1f4ca} ", ""),
            style("Test Summary").bold(),
            style(summary.passed).green().bold(),
            if summary.failed > 0 {
                style(summary.failed).red().bold()
            } else {
                style(summary.failed).green().bold()
            },
            style(summary.skipped).yellow()
        )).ok();
    }

    /// Print per-step duration statistics table.
    pub fn print_stats_table(&self, steps: &[(String, Option<Duration>, StepStatus)]) {
        self.mp.println(format!(
            "\n{}",
            style("\u{2500}".repeat(60)).dim() // ─
        )).ok();
        self.mp.println(format!(
            "  {}  {:<16}{:<12}{}",
            Emoji("\u{23f1}  ", ""),
            style("Step").bold(),
            style("Duration").bold(),
            style("Status").bold()
        )).ok();

        let mut total_duration = Duration::ZERO;
        for (name, duration, status) in steps {
            let dur_str = duration
                .map(|d| {
                    total_duration += d;
                    format_duration(d)
                })
                .unwrap_or_else(|| "-".to_string());
            let status_icon = match status {
                StepStatus::Success => format!("{}", CHECK),
                StepStatus::Failed => format!("{}", CROSS),
                StepStatus::Skipped => "\u{23ed}".to_string(), // ⏭
                StepStatus::Running => "\u{23f3}".to_string(), // ⏳
                StepStatus::Pending => "\u{2b1c}".to_string(), // ⬜
            };
            self.mp.println(format!(
                "     {:<16}{:<12}{}",
                style(name).bold(),
                dur_str,
                status_icon
            )).ok();
        }

        self.mp.println(format!(
            "{}",
            style("\u{2500}".repeat(60)).dim()
        )).ok();
        self.mp.println(format!(
            "     {:<16}{}",
            style("Total").bold(),
            format_duration(total_duration)
        )).ok();
        self.mp.println("".to_string()).ok();
    }

    /// Finish the MultiProgress (must be called to restore terminal).
    pub fn finish(&self) {
        // All bars should already be finished at this point.
    }
}
```

- [ ] 6.3 验证编译通过：`cargo build`
- [ ] 6.4 提交：`git add src/output/tty.rs && git commit -m "feat: add PipelineProgressUI with MultiProgress-based TTY output"`

---

## Task 7: Plain 模式增强 (src/output/plain.rs)

**目标:** 在 plain 模式中添加实时日志打印函数和统计表输出，支持 verbose 控制。

**文件变更:**
- 修改 `src/output/plain.rs`

### 步骤

- [ ] 7.1 重写 `src/output/plain.rs`

将文件内容替换为：

```rust
use std::time::Duration;
use crate::run_state::{RunState, StepStatus};
use crate::strategy::test_parser::TestSummary;

/// Print the run state summary (existing functionality, for status command).
pub fn print_run_state(state: &RunState) {
    println!("Pipeline: {} [{:?}]", state.pipeline, state.status);
    if let Some(ms) = state.duration_ms {
        println!("Duration: {:.1}s", ms as f64 / 1000.0);
    }
    println!();
    for step in &state.steps {
        let icon = match step.status {
            StepStatus::Success => "[OK]",
            StepStatus::Failed => "[FAIL]",
            StepStatus::Skipped => "[SKIP]",
            StepStatus::Running => "[..]",
            StepStatus::Pending => "[--]",
        };
        println!("  {} {} ({})", icon, step.name, step.image);
        if step.status == StepStatus::Failed {
            if let Some(ref stderr) = step.stderr {
                for line in stderr.lines().take(10) {
                    println!("    | {}", line);
                }
            }
        }
    }
}

/// Print step start line in plain mode.
pub fn print_step_start(name: &str, image: &str) {
    println!("[{}] Starting... ({})", name, image);
}

/// Print a realtime log line in plain mode.
/// In non-verbose mode, this should only be called for error lines.
pub fn print_log_line(step_name: &str, message: &str) {
    let trimmed = message.trim_end();
    if !trimmed.is_empty() {
        println!("[{}] {}", step_name, trimmed);
    }
}

/// Print step completion line in plain mode.
pub fn print_step_finish(name: &str, success: bool, duration: Duration) {
    let status = if success { "OK" } else { "FAIL" };
    println!("[{}] {} ({})", name, status, format_duration(duration));
}

/// Print test summary in plain mode (no colors).
pub fn print_test_summary(summary: &TestSummary) {
    println!();
    println!(
        "Test Summary: {} passed, {} failed, {} skipped",
        summary.passed, summary.failed, summary.skipped
    );
}

/// Print per-step duration statistics table in plain mode (no colors).
pub fn print_stats_table(steps: &[(String, Option<Duration>, StepStatus)]) {
    println!();
    println!("{:<16}{:<12}{}", "Step", "Duration", "Status");

    let mut total_duration = Duration::ZERO;
    for (name, duration, status) in steps {
        let dur_str = duration
            .map(|d| {
                total_duration += d;
                format_duration(d)
            })
            .unwrap_or_else(|| "-".to_string());
        let status_str = match status {
            StepStatus::Success => "OK",
            StepStatus::Failed => "FAIL",
            StepStatus::Skipped => "SKIP",
            StepStatus::Running => "RUNNING",
            StepStatus::Pending => "PENDING",
        };
        println!("{:<16}{:<12}{}", name, dur_str, status_str);
    }

    println!();
    println!("{:<16}{}", "Total", format_duration(total_duration));
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
```

- [ ] 7.2 验证编译通过：`cargo build`
- [ ] 7.3 运行测试：`cargo test`
- [ ] 7.4 提交：`git add src/output/plain.rs && git commit -m "feat: add realtime log printing and stats table to plain output mode"`

---

## Task 8: CLI 集成 - 串联所有组件

**目标:** 在 `cmd_run` 和 `cmd_retry` 中根据 OutputMode 创建进度 UI、传递 `on_log` 回调、调用 `parse_test_output()`、输出统计表。

**文件变更:**
- 修改 `src/cli/mod.rs`

### 步骤

- [ ] 8.1 在 `src/cli/mod.rs` 顶部添加新的 imports

在现有 imports 之后添加：

```rust
use crate::output::tty::PipelineProgressUI;
use crate::output::plain as plain_output;
use crate::strategy::test_parser::TestSummary;
use crate::strategy;
use crate::detector::ProjectType;
```

- [ ] 8.2 重写 `cmd_run` 函数核心逻辑

这是最复杂的改动。主要变更点：

**a)** 删除 `let _verbose = verbose;` 行（来自 Task 5）。

**b)** 在创建 `reporter` 之后、进入 batch 循环之前，根据 OutputMode 创建进度 UI：

```rust
    // Collect step names for progress UI
    let step_names: Vec<String> = schedule.iter()
        .flat_map(|batch| batch.iter().cloned())
        .collect();

    // Create progress UI based on output mode
    let mut progress_ui = if mode == OutputMode::Tty {
        Some(PipelineProgressUI::new(&step_names, verbose))
    } else {
        None
    };
```

**c)** 在 batch 循环中，step 开始前通知 UI：

在 `let handles: Vec<_> = batch.iter().map(...)` 之前添加：

```rust
        // Notify UI that steps in this batch are starting
        for step_name in batch {
            if let Some(ref mut ui) = progress_ui {
                ui.start_step(step_name);
            }
            if mode == OutputMode::Plain {
                let image = pipeline.get_step(step_name)
                    .map(|s| s.image.as_str())
                    .unwrap_or("-");
                plain_output::print_step_start(step_name, image);
            }
        }
```

**d)** `tokio::spawn` 内的 `on_log` 闭包目前无法在异步任务中直接更新 progress_ui（因为 `MultiProgress` 不是 Send+Sync 在所有场景下都方便）。推荐方案：使用 `tokio::sync::mpsc` channel 将日志从 executor 发回主线程。

在 batch 循环开头（`for (batch_idx, batch) in schedule.iter().enumerate()` 内部），创建 channel：

```rust
        let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel::<(String, crate::executor::LogLine)>();
```

修改 `tokio::spawn` 中的闭包，改用 channel 发送日志：

```rust
        let handles: Vec<_> = batch
            .iter()
            .map(|step_name| {
                let executor = executor.clone();
                let step = pipeline
                    .get_step(step_name)
                    .expect("step must exist")
                    .clone();
                let pipeline_name = pipeline.name.clone();
                let dir = project_dir.clone();
                let tx = log_tx.clone();
                let sn = step_name.clone();
                tokio::spawn(async move {
                    executor.run_step(&pipeline_name, &step, &dir, move |line| {
                        let _ = tx.send((sn.clone(), line.clone()));
                    }).await
                })
            })
            .collect();
        drop(log_tx); // Drop sender so rx will end when all tasks finish
```

在 handles 和结果收集之间，启动一个异步任务来消费日志 channel（或者在收集结果前 drain channel）。因为需要同时等待 handles 和处理日志，可以使用 `tokio::select!` 或者先 spawn 一个日志消费任务：

```rust
        // Spawn log consumer task
        let is_tty = mode == OutputMode::Tty;
        let is_plain = mode == OutputMode::Plain;
        let log_verbose = verbose;
        let log_consumer = tokio::task::spawn_local(async move {
            // Note: this won't work with spawn_local easily.
            // Alternative: drain after handles complete.
        });
```

**更简洁的方案:** 因为 batch 内的 steps 是并行执行的但结果是顺序收集的，且进度 UI 更新在主线程——一个更实际的做法是在结果收集循环后 drain channel，或者改用非 async 方式。

**推荐简化方案:** 对于 v1，在 `on_log` 中直接使用 `println!`（plain 模式）或不在 tokio::spawn 内更新 MultiProgress（因为 indicatif 的 MultiProgress 是线程安全的可以 clone）。

修改后的完整 batch 处理逻辑：

```rust
    'outer: for (batch_idx, batch) in schedule.iter().enumerate() {
        current_batch_index = batch_idx;

        // Notify UI that steps in this batch are starting
        for step_name in batch {
            if let Some(ref mut ui) = progress_ui {
                ui.start_step(step_name);
            }
            if mode == OutputMode::Plain {
                let image = pipeline.get_step(step_name)
                    .map(|s| s.image.as_str())
                    .unwrap_or("-");
                plain_output::print_step_start(step_name, image);
            }
        }

        let handles: Vec<_> = batch
            .iter()
            .map(|step_name| {
                let executor = executor.clone();
                let step = pipeline
                    .get_step(step_name)
                    .expect("step must exist")
                    .clone();
                let pipeline_name = pipeline.name.clone();
                let dir = project_dir.clone();
                let step_name_for_log = step_name.clone();
                let log_mode = mode.clone();
                let log_verbose = verbose;
                tokio::spawn(async move {
                    let sn = step_name_for_log;
                    executor.run_step(&pipeline_name, &step, &dir, |line| {
                        // Plain mode: print log lines in realtime
                        if log_mode == OutputMode::Plain {
                            if log_verbose || line.stream == crate::executor::LogStream::Stderr {
                                plain_output::print_log_line(&sn, &line.message);
                            }
                        }
                        // TTY mode: handled via progress_ui after result collection
                        // JSON mode: no-op
                    }).await
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let result = handle.await??;
            let step_name = &batch[i];
            let pipeline_step = pipeline.get_step(step_name);

            // Finish step in TTY progress UI
            if let Some(ref mut ui) = progress_ui {
                ui.finish_step(step_name, result.success, result.duration);
            }
            if mode == OutputMode::Plain {
                plain_output::print_step_finish(step_name, result.success, result.duration);
            }

            // ... (existing on_failure_state, step_status, StepState construction code stays the same)
```

**e)** 在 StepState 构造中，对名为 "test" 的 step 调用 parse_test_output()。在构造 StepState 之前：

```rust
            // Parse test summary if this is a test step
            let test_summary = if step_name == "test" || step_name.contains("test") {
                // We need the project type to get the right strategy
                // For now, try to parse with the output directly
                // This will be enhanced when we have project type context
                None // Placeholder - will be filled in step 8.3
            } else {
                None
            };
```

然后将 StepState 构造中的 `test_summary: None,` 改为 `test_summary,`。

**f)** 为了获取 `parse_test_output()` 功能，需要在 `cmd_run` 中确定项目类型。在 pipeline 加载之后添加逻辑：尝试检测项目类型。

在 `let pipeline = Pipeline::from_file(&file)` 之后添加：

```rust
    // Try to detect project type for test output parsing
    let detected_strategy: Option<Box<dyn crate::strategy::PipelineStrategy>> = {
        if let Ok((info, _)) = crate::detector::detect_and_generate(&project_dir) {
            Some(crate::strategy::strategy_for_type(&info.project_type))
        } else {
            None
        }
    };
```

注意：`strategy_for()` 当前是 `fn strategy_for` 在 `strategy/mod.rs` 中但不是 pub。需要将其改为 pub 或者添加一个 pub wrapper。

在 `src/strategy/mod.rs` 中，将 `fn strategy_for` 改为 `pub fn strategy_for`（或添加一个 pub 函数）：

```rust
pub fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
```

也添加一个方便的别名函数供外部使用：

```rust
/// Get a strategy for a given project type (public alias).
pub fn strategy_for_type(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    strategy_for(project_type)
}
```

然后在 test_summary 解析处：

```rust
            let test_summary = if step_name == "test" || step_name.contains("test") {
                let full_output = format!(
                    "{}{}",
                    result.stdout_string(),
                    result.stderr_string()
                );
                detected_strategy.as_ref().and_then(|s| s.parse_test_output(&full_output))
            } else {
                None
            };
```

**g)** 在 batch 循环结束后、输出最终结果之前，收集统计数据并打印：

```rust
    // Collect test summaries and step stats for final output
    let mut aggregated_test_summary: Option<TestSummary> = None;
    let mut step_stats: Vec<(String, Option<std::time::Duration>, StepStatus)> = Vec::new();

    for step_state in &state.steps {
        let duration = step_state.duration_ms.map(|ms| std::time::Duration::from_millis(ms));
        step_stats.push((
            step_state.name.clone(),
            duration,
            step_state.status.clone(),
        ));

        if let Some(ref ts) = step_state.test_summary {
            if let Some(ref mut agg) = aggregated_test_summary {
                agg.passed += ts.passed;
                agg.failed += ts.failed;
                agg.skipped += ts.skipped;
            } else {
                aggregated_test_summary = Some(ts.clone());
            }
        }
    }

    // Output based on mode
    match mode {
        OutputMode::Json => json::print_run_state(&state),
        OutputMode::Plain => {
            if let Some(ref summary) = aggregated_test_summary {
                plain_output::print_test_summary(summary);
            }
            plain_output::print_stats_table(&step_stats);
        }
        OutputMode::Tty => {
            if let Some(ref ui) = progress_ui {
                if let Some(ref summary) = aggregated_test_summary {
                    ui.print_test_summary(summary);
                }
                ui.print_stats_table(&step_stats);
                ui.finish();
            }
        }
    }
```

这段代码替换原来的 `match mode { ... }` 输出块。

- [ ] 8.3 修改 `src/strategy/mod.rs`，将 `strategy_for` 改为 pub

将：

```rust
fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
```

改为：

```rust
pub fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
```

- [ ] 8.4 在 `cmd_retry` 中应用类似改动

`cmd_retry` 的改动较小，主要是：
- 删除 `let _verbose = verbose;`
- 在 `run_step` 调用中传递实际的 on_log 回调（plain 模式打印，其他 no-op）
- step 完成后解析 test_summary 并写入 StepState
- 最终输出统计表

在 `cmd_retry` 中第一个 `run_step` 调用处：

```rust
    let sn = step_name.clone();
    let log_mode = mode.clone();
    let log_verbose = verbose;
    let result = executor.run_step(&pipeline.name, pipeline_step, &project_dir, |line| {
        if log_mode == OutputMode::Plain {
            if log_verbose || line.stream == crate::executor::LogStream::Stderr {
                plain_output::print_log_line(&sn, &line.message);
            }
        }
    }).await?;
```

在 retry 后更新 step state 时，添加 test_summary 解析：

```rust
    // Parse test summary for retried step
    {
        let ss = state.get_step_mut(&step_name).expect("step must exist in state");
        if step_name == "test" || step_name.contains("test") {
            let full_output = format!(
                "{}{}",
                result.stdout_string(),
                result.stderr_string()
            );
            if let Ok((info, _)) = crate::detector::detect_and_generate(&project_dir) {
                let strategy = crate::strategy::strategy_for(&info.project_type);
                ss.test_summary = strategy.parse_test_output(&full_output);
            }
        }
    }
```

类似地，在 skipped step 重执行循环中也需要更新 on_log 和 test_summary。

在 `cmd_retry` 的最终输出部分，替换原来的 `match mode { ... }` 为带统计表的版本（同 cmd_run 中的逻辑）。

- [ ] 8.5 验证编译通过：`cargo build`
- [ ] 8.6 运行所有测试：`cargo test`
- [ ] 8.7 提交：`git add src/cli/mod.rs src/strategy/mod.rs src/output/ && git commit -m "feat: wire realtime output, test parsing, and stats table into CLI"`

---

## Task 9: 端到端验证

**目标:** 完整编译、运行全部测试套件，确认没有回归。

### 步骤

- [ ] 9.1 完整编译：`cargo build`
- [ ] 9.2 运行全部测试：`cargo test`
- [ ] 9.3 运行 clippy 检查：`cargo clippy -- -D warnings`
- [ ] 9.4 格式化检查：`cargo fmt -- --check`
- [ ] 9.5 如果有 clippy/fmt 问题，修复并提交
- [ ] 9.6 手动验证：在一个包含 `pipeline.yml` 的真实项目目录下运行 `cargo run -- run --verbose`，验证 TTY 输出、统计表正常显示
- [ ] 9.7 验证 plain 模式：`cargo run -- run --output plain --verbose`
- [ ] 9.8 验证 JSON 模式：`cargo run -- run --output json`，确认 `test_summary` 字段出现在输出中
- [ ] 9.9 最终提交（如有修复）：`git commit -m "fix: address clippy warnings and formatting in realtime output feature"`

---

## 依赖关系

```
Task 1 (TestSummary + trait)
    ├── Task 2 (6 语言解析器) ← 依赖 Task 1
    └── Task 3 (StepState 字段) ← 依赖 Task 1
Task 4 (Executor 回调) ← 独立
Task 5 (--verbose) ← 独立
Task 6 (TTY rewrite) ← 依赖 Task 1 (需要 TestSummary)
Task 7 (Plain rewrite) ← 依赖 Task 1 (需要 TestSummary)
Task 8 (CLI 集成) ← 依赖 Task 2, 3, 4, 5, 6, 7 (全部)
Task 9 (E2E 验证) ← 依赖 Task 8
```

可并行的任务组:
- **并行组 A:** Task 2 + Task 3（都只依赖 Task 1）
- **并行组 B:** Task 4 + Task 5（互相独立，也不依赖 Task 1）
- **并行组 C:** Task 6 + Task 7（都只依赖 Task 1）

最优执行顺序: Task 1 → (Task 2 | Task 3 | Task 4 | Task 5 | Task 6 | Task 7) → Task 8 → Task 9
