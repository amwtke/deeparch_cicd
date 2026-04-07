# Architecture

## 调用链路

```
用户输入: pipelight run / pipelight init
          │
          ▼
┌─────────────────┐
│   CLI (clap)    │  src/cli/mod.rs
│                 │  解析命令行参数
│                 │  分发到 cmd_run / cmd_init / cmd_retry / cmd_status
└────────┬────────┘
         │
         │ init 命令                          run/retry 命令
         ▼                                         │
┌─────────────────┐                                │
│   Detector      │  src/detector/                 │
│                 │  扫描项目目录                   │
│  MavenDetector  │  检测 pom.xml/Cargo.toml 等    │
│  GradleDetector │  输出 ProjectInfo              │
│  RustDetector   │  (image, build_cmd, test_cmd)  │
│  NodeDetector   │                                │
│  PythonDetector │                                │
│  GoDetector     │                                │
└────────┬────────┘                                │
         │                                         │
         ▼                                         │
┌─────────────────┐                                │
│  PipelineGen    │  src/pipeline_gen/             │
│                 │  接收 ProjectInfo               │
│  BaseStrategy   │  生成 Vec<StepDef>             │
│  MavenStrategy  │  转换为 Pipeline               │
│  GradleStrategy │  写出 pipeline.yml             │
│  RustStrategy   │                                │
│  NodeStrategy   │                                │
│  PythonStrategy │                                │
│  GoStrategy     │                                │
└─────────────────┘                                │
                                                   │
         ┌─────────────────────────────────────────┘
         │  读取 pipeline.yml
         ▼
┌─────────────────┐
│   Pipeline      │  src/pipeline/mod.rs
│                 │  YAML 解析 → Pipeline { steps }
│                 │  验证: 重复名、依赖存在、无自环
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Scheduler     │  src/scheduler/mod.rs
│                 │  用 petgraph 构建 DAG
│                 │  拓扑排序 → Vec<Vec<step_name>>
│                 │  每层(batch)内的 step 可并行
│                 │  输出: [[build], [test, lint], [package]]
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Executor      │  src/executor/mod.rs
│                 │  按 batch 顺序执行
│  DockerExecutor │  每个 step → 创建 Docker 容器
│                 │  bind mount 项目目录 → /workspace
│                 │  收集 stdout/stderr → StepResult
│                 │  返回 exit_code + logs + duration
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Output        │  src/output/
│                 │
│  tty.rs         │  彩色终端输出，emoji，给人看
│  plain.rs       │  纯文本无颜色，管道/重定向/CI
│  json.rs        │  结构化 JSON，给 Claude Code headless
│  mod.rs         │  OutputMode 枚举 + 自动检测
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   RunState      │  src/run_state/mod.rs
│                 │  每次 run 的状态持久化
│                 │  保存到 ~/.pipelight/runs/<id>/status.json
│                 │  retry 命令读取状态恢复执行
└─────────────────┘
```

## 数据流

```
init:  目录 → Detector → ProjectInfo → PipelineGen → Pipeline → pipeline.yml
run:   pipeline.yml → Pipeline → Scheduler(DAG) → Executor(Docker) → StepResult → Output + RunState
retry: RunState + Pipeline → Executor → StepResult → Output + RunState
```

## 模块职责

### CLI (src/cli/mod.rs)
- clap 命令定义: run, validate, list, retry, init, status
- 参数解析和命令分发
- 协调各模块完成命令执行

### Detector (src/detector/)
- 策略模式: ProjectDetector trait
- base/mod.rs: 抽象层 — ProjectDetector trait, ProjectInfo, ProjectType, 检测编排 (detect_and_generate)
- 每种语言一个 detector 文件 (maven.rs, gradle.rs, rust_project.rs, node.rs, python.rs, go.rs) — 策略层
- 职责: 回答「这是什么项目」— 检测项目类型、提取语言版本、框架信息
- 输出: ProjectInfo (image, build_cmd, test_cmd, lint_cmd, fmt_cmd, source_paths, config_files)

### PipelineGen (src/pipeline_gen/)
- 策略模式: PipelineStrategy trait
- base/mod.rs: BaseStrategy 提供 build/test/lint/fmt 四个标准 step 工厂方法
- 每种语言一个策略目录，可覆盖 base step 或添加特有 step
- 语言特有 step 每个一个文件 (如 maven/checkstyle.rs, maven/package.rs)
- 职责: 回答「这个项目该怎么跑 CI」— 生成 step 列表和依赖关系
- 输出: Pipeline (name + steps)

### Pipeline (src/pipeline/mod.rs)
- 数据模型: Pipeline, Step, OnFailure, Strategy
- YAML 序列化/反序列化 (serde + serde_yaml)
- 验证: step 名称唯一、depends_on 引用存在、无自依赖

### Scheduler (src/scheduler/mod.rs)
- 用 petgraph 构建 DAG
- 拓扑排序生成执行计划: Vec<Vec<String>> (batch 列表)
- 同一 batch 内的 step 无依赖，可并行执行

### Executor (src/executor/mod.rs)
- 通过 bollard crate 与 Docker API 交互 (不 shell 调用 docker CLI)
- 创建容器、bind mount 项目目录、执行命令、收集日志
- 输出: StepResult (exit_code, logs, duration, success)

### Output (src/output/)
- **tty.rs**: 彩色终端输出，emoji 图标，进度条 (indicatif)，给人在终端交互使用
- **plain.rs**: 纯文本输出，无 ANSI 转义码，用于管道/重定向/CI 环境
- **json.rs**: 结构化 JSON 输出，给 Claude Code headless 模式或其他程序消费
- **mod.rs**: OutputMode 枚举 (Tty/Plain/Json)，自动检测 stdout 是否为 TTY

### RunState (src/run_state/mod.rs)
- 运行状态模型: RunState, StepState, StepStatus
- 持久化到 ~/.pipelight/runs/<run-id>/status.json
- 支持 retry 命令恢复执行失败的 step

## 关键依赖

| Crate | Purpose |
|-------|---------|
| clap | CLI 参数解析 |
| tokio | 异步运行时 |
| serde + serde_yaml | YAML 配置解析 |
| bollard | Docker API (不 shell 调用 docker CLI) |
| petgraph | DAG 构建和调度 |
| anyhow + thiserror | 错误处理 |
| tracing | 结构化日志 |
| indicatif | 终端进度条 |
| console | 终端颜色和样式 |
| futures-util | Docker 日志流处理 |

## 设计原则

- **本地优先**: 在开发机上运行，不依赖外部服务
- **Docker 隔离**: 所有 step 在容器内执行，环境一致
- **DAG 调度**: 无依赖的 step 可并行执行
- **策略模式**: detector 和 strategy 都用策略模式，易于扩展新语言
- **三种输出模式**: TTY (人)、Plain (CI)、JSON (LLM agent)
- **失败可恢复**: RunState 持久化 + retry 命令
