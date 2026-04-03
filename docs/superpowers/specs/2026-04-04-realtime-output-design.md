# 实时输出增强设计

## 概述

增强 CLI 输出能力：实时日志流、多行进度条、UT 统计解析、step 耗时统计表。所有语言策略通用支持。

## 设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 实时日志粒度 | 默认精简，`--verbose` 全量 | 日常不刷屏，排查问题时看全量 |
| 进度条样式 | 多行固定布局（indicatif MultiProgress） | 一眼看到全局状态 |
| UT 解析方式 | 每个语言策略各自定义解析正则 | 和 strategy 架构契合，精确匹配各语言格式 |
| JSON 模式 UT | 结构化字段写入输出 | Claude Code headless 直接消费 |

## 一、Executor 实时日志回调

### 当前问题

`run_step()` 把容器日志全部攒到 `Vec<LogLine>`，跑完才返回。

### 改造

`run_step()` 新增 `on_log` 回调参数：

```rust
pub async fn run_step(
    &self,
    pipeline_name: &str,
    step: &Step,
    project_dir: &Path,
    on_log: impl Fn(&LogLine),
) -> Result<StepResult>
```

在日志收集循环中，每收到一行同时调用 `on_log` 并 push 到 Vec：

```rust
while let Some(result) = log_stream.next().await {
    match result {
        Ok(output) => {
            let line = LogLine { stream, message };
            on_log(&line);       // 实时推送
            logs.push(line);     // 保留完整日志
        }
        ...
    }
}
```

调用方根据 OutputMode 传入不同闭包：
- **TTY**: 更新 MultiProgress 下方的日志区域
- **Plain**: 直接 println（精简或全量）
- **JSON**: 空闭包 `|_| {}`

## 二、多行进度条（TTY 模式）

### 布局

```
🚀 Pipeline: maven-java-ci (3 steps)
────────────────────────────────────────────────────────
✅ build        47.4s
⏳ test         12.3s  Running...
   │ [INFO] Running com.example.UserTest
   │ [INFO] Tests run: 20, Failures: 0
⬚  package      -
```

### 实现

用 `indicatif::MultiProgress` + 每个 step 一个 `ProgressBar`：

- 初始化时为所有 step 创建 ProgressBar，显示为 `⬚ step_name  -`
- step 开始时切换为 spinner 样式：`⏳ step_name  0.0s  Running...`
- step 运行中每秒更新耗时
- 当前 step 下方显示最近 3 行日志（精简模式），`--verbose` 不限制
- step 完成时 finish 为固定文本：`✅ step_name  47.4s` 或 `❌ step_name  5.2s`

### 文件

重写 `src/output/tty.rs`，新建 `PipelineProgressUI` 结构体管理 MultiProgress 生命周期。

## 三、Plain 模式

逐行打印，无 ANSI 颜色，无进度条刷新：

```
[build] Starting... (maven:3.9-eclipse-temurin-8)
[build] [INFO] Compiling 42 source files
[build] OK (47.4s)
[test] Starting... (maven:3.9-eclipse-temurin-8)
[test] OK (1m 23s)
[package] Starting... (maven:3.9-eclipse-temurin-8)
[package] FAIL (5.2s)
```

精简模式下只打印 start/end 行和错误行。`--verbose` 时打印所有日志行。

## 四、UT 解析

### trait 扩展

在 `PipelineStrategy` trait 上新增方法（带默认实现）：

```rust
pub struct TestSummary {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

pub trait PipelineStrategy {
    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef>;
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
    fn parse_test_output(&self, output: &str) -> Option<TestSummary> {
        None
    }
}
```

### 各语言解析规则

| 语言 | 正则 | 示例输出 | 特殊处理 |
|------|------|---------|---------|
| Maven | `Tests run: (\d+), Failures: (\d+), Errors: (\d+), Skipped: (\d+)` | Tests run: 42, Failures: 0, Errors: 0, Skipped: 2 | 多模块累加所有匹配行 |
| Gradle | `(\d+) tests completed, (\d+) failed` | 42 tests completed, 0 failed | - |
| Rust | `test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored` | test result: ok. 10 passed; 0 failed; 0 ignored | - |
| Node/Jest | `Tests:\s+(?:(\d+) failed, )?(\d+) passed` 或 mocha `(\d+) passing` | Tests: 2 failed, 8 passed | 兼容 jest 和 mocha |
| Python/pytest | `(\d+) passed(?:.*?(\d+) failed)?(?:.*?(\d+) skipped)?` | 5 passed, 1 failed, 2 skipped | - |
| Go | 统计 `^ok\s+` 行和 `^FAIL\s+` 行数量 | ok pkg 0.1s / FAIL pkg 0.2s | 每行一个 package |

### TestSummary 文件

新建 `src/strategy/test_parser.rs`，定义 `TestSummary` 结构体。各语言策略在自己的 mod.rs 里实现 `parse_test_output()`。

## 五、统计表

pipeline 执行结束后打印：

### TTY 模式

```
────────────────────────────────────────────────────────
📊 Test Summary: 42 passed, 0 failed, 2 skipped

⏱  Step         Duration    Status
   build        47.4s       ✅
   test         1m 23s      ✅
   package      5.2s        ❌
────────────────────────────────────────────────────────
   Total        2m 15.8s
```

### Plain 模式

```
Test Summary: 42 passed, 0 failed, 2 skipped

Step         Duration    Status
build        47.4s       OK
test         1m 23s      OK
package      5.2s        FAIL

Total        2m 15.8s
```

### JSON 模式

StepState 新增 test_summary 字段：

```json
{
  "pipeline": "maven-java-ci",
  "status": "Success",
  "duration_ms": 135800,
  "steps": [
    {
      "name": "build",
      "status": "Success",
      "duration_ms": 47400
    },
    {
      "name": "test",
      "status": "Success",
      "duration_ms": 83200,
      "test_summary": {
        "passed": 42,
        "failed": 0,
        "skipped": 2
      }
    },
    {
      "name": "package",
      "status": "Failed",
      "duration_ms": 5200
    }
  ]
}
```

## 六、`--verbose` 参数

在 CLI 的 `Run` 和 `Retry` 子命令上添加 `--verbose` flag：

```rust
#[arg(long)]
verbose: bool,
```

传递给 output 层，控制实时日志的显示量：
- 不加 `--verbose`: 只显示最近 3 行日志（TTY）或 start/end + error 行（Plain）
- 加 `--verbose`: 全量实时输出所有日志行

## 七、文件变更清单

| 操作 | 文件 | 变更内容 |
|------|------|---------|
| 新增 | `src/strategy/test_parser.rs` | TestSummary 结构体 |
| 修改 | `src/strategy/mod.rs` | pub mod test_parser, trait 加 parse_test_output() |
| 修改 | `src/strategy/maven/mod.rs` | 实现 parse_test_output() |
| 修改 | `src/strategy/gradle/mod.rs` | 实现 parse_test_output() |
| 修改 | `src/strategy/rust_lang/mod.rs` | 实现 parse_test_output() |
| 修改 | `src/strategy/node/mod.rs` | 实现 parse_test_output() |
| 修改 | `src/strategy/python/mod.rs` | 实现 parse_test_output() |
| 修改 | `src/strategy/go/mod.rs` | 实现 parse_test_output() |
| 重写 | `src/output/tty.rs` | PipelineProgressUI, MultiProgress 多行进度条, 实时日志, 统计表 |
| 修改 | `src/output/plain.rs` | 实时日志输出, 统计表 |
| 修改 | `src/output/json.rs` | 无变化（已经序列化 RunState） |
| 修改 | `src/output/mod.rs` | OutputMode 无变化 |
| 修改 | `src/executor/mod.rs` | run_step 加 on_log 回调参数 |
| 修改 | `src/cli/mod.rs` | --verbose 参数, on_log 闭包, 调用 parse_test_output(), 统计表输出 |
| 修改 | `src/run_state/mod.rs` | StepState 加 test_summary 可选字段 |

## 八、测试策略

- **UT 解析单元测试**: 每个语言策略的 parse_test_output() 用真实样本输出测试，含多模块场景
- **StepDef/TestSummary 单元测试**: 验证结构体 Default, From 等
- **集成测试**: 端到端 init → run，验证 TTY/Plain 输出包含统计信息
- **JSON 输出测试**: 验证 test_summary 字段正确序列化
