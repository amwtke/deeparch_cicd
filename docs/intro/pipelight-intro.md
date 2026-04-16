# Pipelight 技术介绍

> 一个用 Rust 实现的轻量级 CI/CD CLI 工具，通过 LLM-in-the-loop 实现自动修复

---

## 1. 使用方法

### 1.1 安装

```bash
# 克隆仓库并编译
git clone git@github.com:amwtke/deeparch_cicd.git
cd deeparch_cicd
cargo build --release

# 安装到 PATH
cp target/release/pipelight ~/.cargo/bin/
```

### 1.2 基本工作流

```bash
# 1) 自动检测项目类型，生成 pipeline.yml
pipelight init -d /path/to/your/project

# 2) 查看执行计划（不实际运行）
pipelight run --dry-run

# 3) 运行流水线
pipelight run --output json

# 4) 重试失败的步骤
pipelight retry --run-id <id> --step <name> --output json
```

### 1.3 支持的项目类型

| 项目类型 | 检测标志 | Docker 镜像 |
|---------|---------|------------|
| Maven/Java | `pom.xml` | `maven:3.9-eclipse-temurin-17` |
| Gradle/Java | `build.gradle` / `build.gradle.kts` | `gradle:8-jdk17` |
| Rust | `Cargo.toml` | `rust:latest` |
| Node.js | `package.json` | `node:20` |
| Vue.js | `package.json` + vue 依赖 | `node:20` |
| Python | `requirements.txt` / `pyproject.toml` | `python:3.12` |
| Go | `go.mod` | `golang:1.22` |

### 1.4 典型 pipeline.yml 结构

```yaml
name: maven-java-ci
git_credentials:
  username: your_username
  password: your_token
steps:
  - name: ping-pong        # 通信测试（默认关闭）
    active: false
  - name: git-pull          # 拉取最新代码
    depends_on: [ping-pong]
  - name: git-diff          # 检测变更文件
    depends_on: [git-pull]
  - name: build             # 编译
    depends_on: [git-diff]
  - name: test              # 测试（allow_failure: true）
    depends_on: [build]
  - name: pmd               # 静态分析
    depends_on: [build]
  - name: spotbugs          # 缺陷检测
    depends_on: [build]
```

### 1.5 CLI 子命令一览

| 子命令 | 用途 |
|-------|------|
| `init` | 自动检测项目类型，生成 pipeline.yml |
| `run` | 运行流水线 |
| `retry` | 重试失败的步骤（含级联执行后续步骤） |
| `validate` | 校验 pipeline.yml 格式与 DAG 合法性 |
| `list` | 列出流水线所有步骤 |
| `status` | 查询指定 run_id 的执行状态 |
| `clean` | 删除 pipeline.yml 和 pipelight-misc/ |
| `docker-prepare` | 预拉取所有 Docker 镜像 |

---

## 2. 设计架构：三个角色

Pipelight 系统由三个角色组成，各自职责清晰分离：

### 2.1 角色定义

```
┌─────────────────────────────────────────────────────────┐
│                    LLM (Claude Code)                     │
│  职责：编排决策者                                          │
│  - 调用 pipelight 二进制执行流水线                         │
│  - 解析 JSON 输出，按 on_failure.action 分发回调           │
│  - 执行智能操作：修复代码 / 生成配置 / 打印报告             │
│  - 决定是否 retry                                        │
├─────────────────────────────────────────────────────────┤
│                 Pipelight 二进制                          │
│  职责：调度执行器                                          │
│  - 检测项目类型 → 生成 pipeline.yml                       │
│  - DAG 拓扑排序 → 按依赖顺序调度步骤                      │
│  - 通过 Docker API (bollard) 启动容器执行命令              │
│  - 收集 stdout/stderr → 匹配异常模式 → 生成 on_failure    │
│  - 输出结构化 JSON，**不做任何智能决策**                    │
├─────────────────────────────────────────────────────────┤
│                   Docker 容器                             │
│  职责：隔离执行环境                                        │
│  - 每个 step 在独立容器中执行                              │
│  - 项目目录通过 bind mount 挂载到 /workspace              │
│  - step 间通过共享 volume 传递 artifact                   │
│  - 容器执行完毕后自动清理                                  │
└─────────────────────────────────────────────────────────┘
```

### 2.2 为什么要分离三个角色？

| 设计原则 | 说明 |
|---------|------|
| **pipelight 不做智能决策** | 它只负责"执行"和"报告"，不搜索文档、不修改代码、不判断修复方案 |
| **LLM 不直接操作 Docker** | 它只通过 pipelight 的 CLI 接口间接控制容器，避免权限泄露 |
| **Docker 只是执行沙箱** | 提供环境隔离和可复现性，不参与任何业务逻辑 |

这种分离让每个角色可以独立升级：
- 换 LLM（从 Claude 到 GPT）只需改 harness skill
- 换容器运行时（从 Docker 到 Podman）只需改 executor 层
- 加新语言支持只需加 detector + pipeline_builder 策略

### 2.3 代码架构分层

```
src/
  main.rs                       ← 入口
  cli/mod.rs                    ← clap 命令定义 + dispatch 逻辑
  run_state/mod.rs              ← RunState 持久化（JSON 文件）
  ci/
    detector/                   ← 项目类型检测（策略模式）
      base/mod.rs               ←   ProjectDetector trait, ProjectType 枚举
      maven.rs / gradle.rs / …  ←   各语言检测策略
    pipeline_builder/           ← Step 生成（StepDef trait + 策略模式）
      base/                     ←   公共 step: ping_pong / git_pull / git_diff / build / test
      maven/ / gradle/ / …      ←   各语言特有 step (pmd / spotbugs 等)
    parser/mod.rs               ← Pipeline YAML 解析 & 校验
    scheduler/mod.rs            ← DAG 构建 (petgraph) & 拓扑排序
    executor/mod.rs             ← Docker 容器执行 (bollard crate)
    callback/
      command.rs                ← CallbackCommand 枚举（语义：发生了什么）
      action.rs                 ← CallbackCommandAction 枚举（语义：LLM 该做什么）
    output/
      json.rs                   ← JSON 输出格式
      plain.rs                  ← 纯文本输出格式
      tty/                      ← 交互式终端 UI（进度条 + 彩色输出）
```

---

## 3. 三角色通信协议

### 3.1 整体协议流程

```
   LLM                      Pipelight                   Docker
    │                           │                          │
    │  pipelight run --json     │                          │
    │ ────────────────────────► │                          │
    │                           │  docker create + start   │
    │                           │ ───────────────────────► │
    │                           │                          │ (执行 shell 命令)
    │                           │  stdout / stderr / exit  │
    │                           │ ◄─────────────────────── │
    │                           │  docker rm               │
    │                           │ ───────────────────────► │
    │                           │                          │
    │                           │ [异常匹配: exit_code +   │
    │                           │  stdout/stderr → 确定    │
    │                           │  CallbackCommand]        │
    │                           │                          │
    │  JSON { status, steps,    │                          │
    │    on_failure { command,   │                          │
    │    action, context_paths }}│                          │
    │ ◄──────────────────────── │                          │
    │                           │                          │
    │ [按 action 分发回调]       │                          │
    │ [修复代码 / 生成配置]      │                          │
    │                           │                          │
    │  pipelight retry --json   │                          │
    │ ────────────────────────► │         ...              │
```

### 3.2 CallbackCommand → CallbackCommandAction 映射

Pipelight 内部通过 `CallbackCommandRegistry` 将具体的 Command 翻译为抽象的 Action：

| CallbackCommand | CallbackCommandAction | LLM 具体操作 |
|----------------|----------------------|-------------|
| `auto_fix` | `retry` | 读 stderr + context_paths 中的源码，修复后 retry |
| `auto_gen_pmd_ruleset` | `retry` | 搜索项目的 PMD 规则配置，生成 pmd-ruleset.xml，retry |
| `ping` | `retry` | 打印 "pong"，直接 retry（通信测试） |
| `git_fail` | `skip` | 无操作，pipelight 已自动标记 skipped |
| `fail_and_skip` | `skip` | 无操作，前置条件缺失，自动 skip |
| `runtime_error` | `runtime_error` | Pipeline 终止，报告错误 |
| `abort` | `abort` | 严重代码问题，Pipeline 终止 |
| `test_print_command` | `test_print` | 解析 JUnit XML，按模块聚合打印测试结果表格 |
| `pmd_print_command` | `pmd_print` | 解析 PMD XML，打印分类违规统计表 |
| `spotbugs_print_command` | `spotbugs_print` | 解析 SpotBugs XML，打印分类缺陷统计表 |
| `git_diff_command` | `git_diff_report` | 读文件列表，打印 unstaged/staged/unpushed 分组 |

### 3.3 on_failure JSON 结构

每个 step 执行完毕后，JSON 输出中的 `on_failure` 字段携带完整的回调信息：

```json
{
  "on_failure": {
    "exception_key": "pmd_violations",
    "command": "auto_fix",
    "action": "retry",
    "max_retries": 9,
    "retries_remaining": 8,
    "context_paths": [
      "pipelight-misc/pmd-report/pmd-result.xml",
      "src/main/java/**/*.java"
    ]
  }
}
```

**关键设计**：
- `command` = 发生了什么事（溯源用）
- `action` = LLM 该做什么类别的动作（分发键）
- `context_paths` = LLM 需要读取的文件路径（提供上下文）

### 3.4 异常匹配机制 (ExceptionMapping)

Pipelight 不是简单地根据 exit code 决定回调，而是通过 `StepDef::match_exception()` 分析 stdout/stderr 内容：

```
exit_code ≠ 0
    │
    ▼
StepDef::match_exception(exit_code, stdout, stderr)
    │
    ├── stderr 包含 "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset"
    │   → exception_key = "ruleset_not_found"
    │   → command = AutoGenPmdRuleset
    │
    ├── stderr 包含 "Cannot load ruleset"
    │   → exception_key = "ruleset_invalid"
    │   → command = AutoGenPmdRuleset
    │
    ├── stdout 包含 "PMD Total: N violations"
    │   → exception_key = "pmd_violations"
    │   → command = AutoFix
    │
    └── 默认
        → command = Abort (或 step 配置的默认 command)
```

这让同一个 step 在不同失败原因下触发不同的回调命令。

### 3.5 LLM 分发规则（强制）

**每次 pipelight 返回 JSON 后，LLM 必须遍历所有 step 的 `on_failure` 字段**：
- 不论 step 的 status 是 success / failed / retryable
- 不论 pipeline 整体 status
- 只要 `on_failure` 非 null，就按 `action` 执行对应操作

这是因为 `test_print` / `pmd_print` / `spotbugs_print` 等"打印型 action"常搭在 `status: success` 的 step 上（如 `allow_failure: true` 的 test step）。

---

## 4. Pipelight 二进制的三种运行模式

### 4.1 初始执行模式 (Run)

```bash
pipelight run -f pipeline.yml --output json [--skip step1,step2] [--ping-pong] [--full-report-only]
```

**流程**：
1. 加载 pipeline.yml
2. 构建 DAG，拓扑排序确定执行顺序
3. 按批次（batch）执行 step：无依赖关系的 step 可并行
4. 每个 step 在 Docker 容器中执行
5. 收集结果，匹配异常模式，生成 `on_failure`
6. 输出完整 JSON（RunState）

**退出码**：
- `0` = 全部成功
- `1` = 有可重试的失败（retryable）
- `2` = 有不可恢复的失败（failed）

### 4.2 重试模式 (Retry)

```bash
pipelight retry --run-id <id> --step <name> --output json
```

**流程**：
1. 从 `~/.pipelight/runs/<run_id>.json` 加载上次运行状态
2. 验证指定 step 是 Failed 状态且 retries_remaining > 0
3. 递减 retries_remaining
4. 重新执行该 step
5. **重新解析** on_failure（`reresolve_on_failure`），而非沿用上次的状态
6. 如果成功，**级联执行**被上游阻塞的后续 step
7. 更新并保存 RunState

**级联 Retry 的三种 Skipped 判定**：

| 情况 | 示例 | 是否级联 |
|------|------|---------|
| `active: false`（配置关闭） | ping-pong 默认不激活 | 不级联 |
| 执行后被 Skip 策略跳过 | git-pull 失败 → git_fail → skip | 不级联 |
| 被上游阻塞而未执行 | build 失败 → test/pmd/spotbugs 未执行 | **级联** |

### 4.3 检查模式 (Dry-Run / Validate / Status / List)

```bash
pipelight run --dry-run       # 显示执行计划，不实际运行
pipelight validate            # 校验 YAML 格式 + DAG 无环
pipelight status --run-id <id> # 查询历史运行状态
pipelight list                # 列出所有 step 及依赖关系
```

这些模式不启动任何 Docker 容器，只做解析和查询。

---

## 5. Harness 工程在本项目中的体现

### 5.1 什么是 Harness 模式？

Harness（测试线束 / 编排驱动器）是一种设计模式：**一个薄层编排程序控制被测系统的输入输出**。在 pipelight 中：

```
┌─────────────────────────────┐
│    pipelight-run Skill      │  ← Harness（定义在 SKILL.md 中）
│    (运行在 LLM 内部)         │
│                             │
│  ┌───────────────────────┐  │
│  │   pipelight 二进制     │  │  ← 被编排的工具
│  │   (Rust CLI)          │  │
│  └───────────────────────┘  │
└─────────────────────────────┘
```

### 5.2 Harness 的具体职责

**pipelight-run Skill（Harness 侧）负责**：
1. 决定何时调用 `pipelight init` / `pipelight run` / `pipelight retry`
2. 解析 JSON 输出，遍历所有 step 的 `on_failure`
3. 按 `action` 类型分发到对应的处理逻辑
4. 执行"智能"操作（搜索文档、修复代码、生成 PMD ruleset）
5. 决定是否继续 retry 或放弃

**pipelight 二进制（被编排侧）负责**：
1. 检测项目类型
2. 执行 Docker 容器
3. 收集输出、匹配异常模式
4. 以结构化 JSON 报告结果
5. **绝不做任何"智能"决策**

### 5.3 Harness 协议的关键约束

```
pipelight 产出 CallbackCommand     （语义：发生了什么事）
        ↓ CallbackCommandRegistry 翻译
pipelight 产出 CallbackCommandAction（语义：LLM 该做什么类别的动作）
        ↓ 两者都写入 JSON on_failure 字段
LLM 收到后按 action 在 skill 处理表查具体动作
        ↓
LLM 执行对应动作（修代码 / 打印报告 / skip / retry）
```

**pipelight 侧职责**：选对 `CallbackCommand`，确保注册表有对应 `Action`
**LLM 侧职责**：按 `action` 查 skill 表执行；`command` 只是溯源用，`action` 才是分发键

### 5.4 Harness 与传统 CI/CD 的对比

| 维度 | Jenkins / GitLab CI | Pipelight + LLM Harness |
|------|--------------------|-----------------------|
| 失败处理 | 报告失败，等人工修复 | 自动分析 + 修复 + 重试 |
| 配置管理 | 手写 Jenkinsfile / .gitlab-ci.yml | `pipelight init` 自动检测生成 |
| 运行环境 | 需要 CI 服务器 | 本地笔记本即可（Docker） |
| 反馈延迟 | 推送 → 排队 → 执行 → 结果（分钟级） | 本地即时执行（秒级） |
| 修复闭环 | 人看报告 → 改代码 → 重新推送 | LLM 读报告 → 改代码 → 自动 retry |
| 扩展新语言 | 改 CI 脚本 | 加 detector + pipeline_builder 策略 |

### 5.5 Skill Memory：Harness 的持久化学习

pipelight-run skill 内置 `memory/` 目录，记录从历次运行中学到的规则：

```
global-skills/pipelight-run/memory/
  feedback_pipelight_callback_dispatch.md
    → 规则：收到 JSON 后必须先遍历所有 on_failure，逐条按 action 执行
    → 原因：曾因 pipeline 整体 success 就跳过 test_print 和 pmd_print
```

这些 memory 文件随 repo 版本控制，`/pipelight-sync` 自动部署到 `~/.claude/skills/` 和 `~/.trae/skills/`。

---

## 6. 未来改进方向

### 6.1 并行执行

当前 DAG 调度已识别出可并行的 batch，但执行仍是串行。未来可利用 tokio 并发执行同一 batch 内的多个 step。

### 6.2 更多语言支持

当前 detector 支持 7 种项目类型。可扩展：
- C/C++ (CMake / Makefile)
- Swift (Package.swift)
- Kotlin Multiplatform

### 6.3 远程执行

当前只支持本地 Docker。未来可支持：
- 远程 Docker daemon
- Kubernetes Job
- SSH 执行

### 6.4 增量构建优化

利用 git-diff step 的变更文件列表，只重新编译/测试受影响的模块，进一步缩短反馈周期。

### 6.5 多 LLM 后端

当前 harness 协议绑定 Claude Code 的 skill 系统。可以抽象为通用的 LLM agent 协议，支持 GPT / Gemini / 本地模型。

### 6.6 Web Dashboard

提供 Web UI 展示历史运行记录、趋势图、失败热点分析。RunState JSON 文件已是结构化数据，只需加前端。
