# AI-Native CI/CD 设计规格书

## 概述

Pipelight 是一个轻量级 CLI CI/CD 工具，设计目标是与任意 LLM agent 协作。与传统 CI/CD 机械地执行命令、失败即停不同，pipelight 引入了**运行-退出-修复-重试循环**——当 step 失败时，流水线输出结构化数据后退出，由外部 LLM agent（或人）分析、修复、重试。

Pipelight 本身不嵌入任何 LLM。LLM 无关是核心设计原则。

## 设计原则

1. **LLM 无关** — 不依赖任何特定 LLM 厂商。Pipelight 输出结构化 JSON；任何 agent（Claude Code、Cursor、Copilot、自定义脚本）都能消费。
2. **CLI 即接口** — LLM agent 通过 shell 命令调用 pipelight，这是最通用的集成方式。
3. **流水线自描述** — pipeline.yml 中的 `on_failure` 元数据告诉 agent 失败时该怎么做，pipelight 本身不执行修复。
4. **向后兼容** — 没有 `on_failure` 的旧 pipeline.yml 文件行为完全不变（失败 = 终止）。

## 核心创新：运行-退出-修复-重试循环

```
传统 CI/CD：
  运行 → 失败 → 停止 → 人读日志 → 人修代码 → 人重新触发整条流水线

Pipelight：
  运行 → 失败 → 输出 JSON 并退出 → agent 读 JSON → agent 修代码 → 重试 → 从断点恢复运行
                                     ↑_____________________________________________↓
                                                    自动修复循环
```

这个循环是 pipelight 的核心差异化。流水线不是单行道——失败是 agent 介入的起点。

**关键设计决策：运行后退出（run-and-exit），而不是长驻进程。**

LLM agent（Claude Code、Cursor、Copilot）调用 shell 命令时会等待命令结束。如果 pipelight 是长驻进程（暂停等待信号文件），agent 会**死锁**——agent 必须等进程退出才能读到 stdout，所以它永远不知道该去修复什么。

因此，每次 pipelight 调用都是完整的运行-退出周期：
1. `pipelight run` — 执行 step，输出 JSON，**退出**
2. Agent 读 JSON，修复代码
3. `pipelight retry` — 从断点恢复执行，输出 JSON，**退出**
4. 重复直到成功或重试次数耗尽

流水线状态通过 `~/.pipelight/runs/<run-id>/status.json` 在多次调用间持久化。

## 输出模式

三种输出模式，通过 `--output` flag 或自动检测选择：

| 模式 | Flag | 触发条件 | 说明 |
|------|------|---------|------|
| TTY | `--output tty` | stdout 是终端（默认） | 彩色进度条，实时日志流 |
| Plain | `--output plain` | stdout 非终端（自动降级） | 纯文本，无 ANSI 转义码 |
| JSON | `--output json` | 显式指定 | 结构化 JSON，每次运行输出一个完整对象 |

自动检测逻辑：stdout 是 TTY 则默认 `tty`，否则默认 `plain`。`--output` flag 始终覆盖自动检测。

## JSON 输出结构

一次完整的流水线运行结果：

```json
{
  "run_id": "abc123",
  "pipeline": "rust-ci",
  "status": "retryable",
  "duration_ms": 12340,
  "steps": [
    {
      "name": "build",
      "status": "failed",
      "exit_code": 101,
      "duration_ms": 8200,
      "image": "rust:1.78-slim",
      "command": "cargo build --release",
      "stdout": "...",
      "stderr": "error[E0277]: the trait bound...",
      "error_context": {
        "files": ["src/executor/mod.rs"],
        "lines": [93],
        "error_type": "compile_error"
      },
      "on_failure": {
        "strategy": "auto_fix",
        "max_retries": 3,
        "retries_remaining": 3,
        "context_paths": ["src/", "Cargo.toml"]
      }
    },
    {
      "name": "lint",
      "status": "success",
      "exit_code": 0,
      "duration_ms": 4100,
      "image": "rust:1.78-slim",
      "command": "cargo clippy -- -D warnings",
      "stdout": "...",
      "stderr": ""
    },
    {
      "name": "test",
      "status": "skipped",
      "reason": "dependency 'build' failed"
    }
  ]
}
```

### error_context（错误上下文）

Pipelight 会尽力解析已知工具（rustc、gcc、pytest 等）的错误输出，提取：
- `files` — 涉及的源文件
- `lines` — 行号
- `error_type` — 错误分类（compile_error、test_failure、lint_error、runtime_error、unknown）

如果解析失败，`error_context` 为 `null`。这是尽力而为，不保证成功。

### on_failure（失败策略）

从 pipeline.yml 配置中原样透传。Pipelight 自身不解读 `strategy` 的含义——只根据策略决定是保存可重试状态退出（auto_fix）还是永久终止退出（abort/notify）。

## Pipeline YAML 扩展

### on_failure 块

添加到 step 定义中。完全可选——不写则默认 `strategy: abort`。

```yaml
steps:
  - name: build
    image: rust:1.78-slim
    commands:
      - cargo build --release
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml
```

### strategy 取值

| 策略 | 含义 | Pipelight 行为 |
|------|------|---------------|
| `abort` | 失败即停（默认） | 标记 step 失败，跳过下游 step，退出 |
| `auto_fix` | 建议 agent 尝试修复 | 保存可重试状态到 status.json，输出 JSON，退出 |
| `notify` | 通知但不建议修复 | 标记 step 失败，JSON 中包含完整信息，终止退出 |

### 完整示例

```yaml
name: rust-ci

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: "1"

steps:
  - name: build
    image: rust:1.78-slim
    commands:
      - cargo build --release
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml

  - name: lint
    image: rust:1.78-slim
    commands:
      - rustup component add clippy
      - cargo clippy -- -D warnings
    on_failure:
      strategy: auto_fix
      max_retries: 2
      context_paths:
        - src/

  - name: test
    image: rust:1.78-slim
    depends_on: [build]
    commands:
      - cargo test --release
    on_failure:
      strategy: notify

  - name: fmt-check
    image: rust:1.78-slim
    commands:
      - rustup component add rustfmt
      - cargo fmt -- --check
    on_failure:
      strategy: auto_fix
      max_retries: 1
      context_paths:
        - src/

  - name: security-audit
    image: rust:1.78-slim
    depends_on: [build]
    allow_failure: true
    commands:
      - cargo install cargo-audit
      - cargo audit
```

## 重试机制（运行后退出）

### 工作原理

每次调用都是完整的运行-退出周期。没有长驻进程，没有信号文件。

当 `strategy: auto_fix` 的 step 失败时：

1. Pipelight 将流水线状态保存到 `~/.pipelight/runs/<run-id>/status.json`
2. Pipelight 将 JSON 输出到 stdout（包含错误详情和 on_failure 元数据）
3. Pipelight **退出**（exit code 1 = 可重试的失败）
4. 外部 agent（或人）读取 JSON 输出，修复问题
5. Agent 运行 `pipelight retry --run-id <id> --step <name> --output json`
6. Pipelight 启动**新进程**，读取 status.json，重新执行失败的 step
7. 如果 step 通过 → 继续执行下游 step → 输出 JSON → 退出
8. 如果 step 再次失败 → 更新 status.json（递减 retries_remaining）→ 输出 JSON → 退出
9. Agent 重复 4-8，直到成功或 retries_remaining 归零
10. retries_remaining = 0 → 输出 JSON（`"final": true`）→ 退出

### 通过 status.json 持久化状态

```
~/.pipelight/runs/<run-id>/
  status.json       ← 完整的流水线状态，跨调用持久化
```

status.json 包含：
- 哪些 step 成功、失败、被跳过
- 每个 auto_fix step 的剩余重试次数
- 原始流水线配置
- 每个 step 的时间戳和耗时

这个文件是多次调用间流水线状态的**唯一事实来源**。

- `pipelight run` 创建 status.json 并写入初始状态
- `pipelight retry` 读取 status.json，重新执行指定 step，更新 status.json
- `pipelight status` 读取并展示 status.json

### 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 所有 step 成功 |
| 1 | Step 失败，可重试（auto_fix 且剩余重试次数 > 0） |
| 2 | Step 失败，最终失败（abort、notify、或重试次数耗尽） |

LLM agent 可以通过退出码快速决策，无需解析 JSON。

### 运行生命周期

```
pipelight run（进程 1）
    ├─ 创建 ~/.pipelight/runs/<run-id>/status.json
    ├─ 按 DAG 顺序执行 step
    ├─ step "build" 失败（strategy: auto_fix）
    ├─ 保存状态到 status.json
    ├─ 输出 JSON 到 stdout
    └─ 退出（exit code 1）

    agent 读取 JSON，修复代码...

pipelight retry --run-id <id> --step build（进程 2）
    ├─ 读取 status.json，恢复流水线状态
    ├─ 重新执行 step "build" → 仍然失败
    ├─ 递减 retries_remaining（3 → 2）
    ├─ 更新 status.json
    ├─ 输出 JSON 到 stdout
    └─ 退出（exit code 1）

    agent 读取 JSON，再次修复代码...

pipelight retry --run-id <id> --step build（进程 3）
    ├─ 读取 status.json，恢复流水线状态
    ├─ 重新执行 step "build" → 成功！
    ├─ 继续执行：step "test"（之前被跳过）→ 成功
    ├─ 更新 status.json
    ├─ 输出 JSON 到 stdout
    └─ 退出（exit code 0）

    完成！
```

### 完整的 LLM agent 交互时序

```
Claude Code                                    pipelight
    │                                              │
    ├─ Bash: pipelight run ... --output json ──────┤
    │                                              ├─ 运行 lint ✓
    │                                              ├─ 运行 build ✗
    │                                              ├─ 跳过 test（依赖 build）
    │                                              ├─ 写入 status.json
    │◄──────────── JSON 输出（exit 1）──────────────┤
    │                                              │
    ├─ 解析 JSON                                   │
    ├─ 读取错误："trait bound not satisfied"         │
    ├─ 读取 context_paths: [src/, Cargo.toml]      │
    ├─ 读取 src/executor/mod.rs                    │
    ├─ 修改 src/executor/mod.rs（修复错误）          │
    │                                              │
    ├─ Bash: pipelight retry ... --step build ─────┤
    │                                              ├─ 读取 status.json
    │                                              ├─ 重新运行 build ✓
    │                                              ├─ 运行 test ✓（解除阻塞）
    │                                              ├─ 更新 status.json
    │◄──────────── JSON 输出（exit 0）──────────────┤
    │                                              │
    ├─ 解析 JSON：所有 step 通过                     │
    └─ 完成                                        │
```

## CLI 命令

### 现有命令（修改）

```bash
# run：增加 --output 和 --run-id flag
pipelight run -f pipeline.yml --output json --run-id <id>

# --output: tty（默认）| plain | json
# --run-id: 可选，不指定则自动生成 UUID
```

### 新增命令

```bash
# retry：从断点重试失败的 step
pipelight retry --run-id <id> --step <name> --output json

# status：查看某次运行的当前状态
pipelight status --run-id <id>
pipelight status --run-id <id> --output json
```

### LLM agent 工作流

```bash
# 1. 启动流水线（进程运行，输出 JSON，退出）
pipelight run -f pipeline.yml --output json
# → exit code 1：可重试的失败
# → JSON 输出包含 run_id、失败 step 详情、on_failure 元数据

# 2. Agent 读取 JSON 输出，看到 build 失败且策略为 auto_fix

# 3. Agent 分析 error_context，修改源文件

# 4. Agent 触发重试（新进程，读取 status.json，重跑 step，退出）
pipelight retry --run-id <run-id> --step build --output json
# → exit code 0：全部通过，完成
# → exit code 1：仍然失败，重复 2-4
# → exit code 2：重试次数耗尽，放弃
```

## Pipelight 不做什么

- 不调用任何 LLM API
- 不修改源代码
- 不解读 `strategy` 的含义（仅控制退出行为）
- 不需要网络访问（Docker 镜像拉取除外）

Pipelight 是**工具**，不是 **agent**。它执行、报告、退出。智能在外部。
