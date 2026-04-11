# Step 失败控制流：Docker、pipelight、LLM 三方协作

> 这是一份 **Harness Engineering** 实践文档——描述 pipelight 如何把非确定性的 LLM
> 当作一个"受约束的执行单元"嵌进确定性的 CI/CD 控制平面里，让 LLM 可靠、可控、可审计地
> 参与自动化流水线。

## Harness Engineering 视角

所谓 **harness（线束、挽具）**，是把一头难以直接驾驭的"猛兽"安全接入系统的机械装置。
在 LLM 工程里，harness 指的是 **包在 LLM 外面的一层确定性控制平面**——它负责状态管理、
决策路由、重试预算、上下文注入、输出校验与可观测性，让 LLM 只做它擅长的事（读、写、推理），
而不让它决定"这个 pipeline 是否该终止"这种关乎正确性的控制权。

pipelight × Claude Code × pipelight-run skill 的组合就是一个 **教科书式的 LLM harness**：

| Harness 工程要素 | 在本项目里的体现 |
|-----------------|------------------|
| **确定性控制平面** | pipelight（Rust）用状态机 + DAG 调度决定流程走向，LLM 零自主权 |
| **有界自治（bounded autonomy）** | 每个 CallbackCommand 都带 `max_retries`，配额耗尽即 fail-close，无死循环风险 |
| **显式契约（explicit contract）** | pipelight ↔ LLM 的唯一接口是严格的 JSON 输出 + `CallbackCommand` 枚举，没有自由文本指令 |
| **职责隔离（separation of concerns）** | 容器跑命令 / harness 做决策 / LLM 做修复——三层互不越权 |
| **最小权限（least authority）** | LLM 只能读 `context_paths` 列出的文件，只能通过 `pipelight retry` 触发重试 |
| **可重放性（reproducibility）** | 每一步 JSON 都落盘到 `pipelight-misc/`，run_id 贯穿整次执行，随时可 retry |
| **失败分类（failure taxonomy）** | 用 `CallbackCommand` 枚举把失败分成 retry / skip / abort 三个响应族，而不是"遇到报错让 LLM 看着办" |
| **模板化扩展（templating layer）** | Rust 代码是"烘焙模板"，`pipeline.yml` 是"运行时契约"，二者分离让项目级定制零摩擦 |
| **人在回路（human-in-the-loop）** | `pipeline.yml` 对用户透明可编辑，任何 harness 决策都能在 YAML 里核对与覆盖 |

**一个核心判断：** 在 LLM 时代，"让 LLM 更聪明"是次要的，"让 LLM 嵌进一个永不失控的 harness"
才是工程化重点。这份文档描述的就是 pipelight 如何构造这样一个 harness。

---

## 核心原则

**LLM 在 pipeline 中是"被动执行者"——它做什么完全由 pipelight 程序决定。**

这是 harness 的第一性原则：**控制权属于确定性代码，非确定性组件只负责执行**。
一个 step 失败后，pipeline 是终止（abort）、跳过继续（skip）、还是让 LLM 修复后重试（retry），
全部由 pipelight 的 Rust 代码通过 JSON 输出中的 `on_failure.command` 字段告诉 LLM。LLM 侧没有任何
"自主决策逻辑"，它只是按 skill 中定义的「回调命令处理表」把 `CallbackCommand` 枚举值翻译成具体动作。

**要调整一个 step 失败后的行为，大多数情况下只需要改项目根目录下的 `pipeline.yml`——
Rust 代码只是 `pipelight init` 生成 `pipeline.yml` 的模板。** 极少数情况（新增 stderr → exception_key
匹配规则）才需要改 Rust 代码。LLM 的提示词和 skill 基本不用动。

> **Harness 视角看这条原则：** 控制平面（pipelight）和执行平面（LLM）之间有一条"只读的契约线"——
> 控制平面通过 JSON 下发指令，执行平面通过 retry 反馈结果，双方不共享状态、不互相调用
> 内部函数。这种**单向命令 + 结构化反馈**的模式是所有可靠 LLM harness 的共同特征。

---

## pipeline.yml 是"真相来源"，Rust 代码只是模板

> **Harness 视角：** 这里体现的是 **"配置即契约"（configuration-as-contract）**。harness 的
> 运行时行为必须能被一个声明式文件完整描述——这样任何人类工程师或运维都可以在不读 Rust
> 源码的前提下审计、修改、回滚 harness 的决策。`pipeline.yml` 就是这份契约。


项目根目录下的 `pipeline.yml` 包含了 pipelight 运行时的全部可配置项：

| pipeline.yml 字段 | 作用 |
|-------------------|------|
| `steps[*].commands` | Docker 容器里执行的 shell 命令（含所有 `exit 0/1` 的判定逻辑） |
| `steps[*].on_failure.callback_command` | 未命中 exceptions 时的默认 CallbackCommand |
| `steps[*].on_failure.max_retries` | 默认最大重试次数 |
| `steps[*].on_failure.context_paths` | 默认传给 LLM 的上下文路径 |
| `steps[*].on_failure.exceptions` | exception_key → { command, max_retries, context_paths } 的映射表 |
| `steps[*].depends_on` / `image` / `volumes` / `local` / `active` | 其他 step 配置 |

**pipelight 运行时**：
1. executor 把 `commands` 丢给 Docker 容器执行，拿 exit_code
2. Rust 代码 `StepDef::match_exception(exit_code, stdout, stderr)` 按 stderr 特征串匹配 → 得到 exception_key
3. 用 exception_key 去 **pipeline.yml 的 `on_failure.exceptions` 表**查 CallbackCommand（不是去 Rust 代码查）
4. 查不到就用 `on_failure.callback_command` 默认值

**`pipelight init` 生成时**：Rust 代码的 `StepDef::config()` 产出 `commands`，`StepDef::exception_mapping()`
产出 `on_failure.exceptions`，一次性烘焙进 `pipeline.yml`。之后用户可以直接编辑 `pipeline.yml`
对这个项目做定制，**但运行 `pipelight init --reinit` 会用 Rust 模板覆盖用户的手工修改**。

**哪些改动必须动 Rust 代码？**
- 新增一条"stderr 特征串 → exception_key"的匹配规则（`match_exception` 的 if/else 是 hardcode 的，
  pipeline.yml 没有地方配它）
- 新增 `CallbackCommand` 枚举值
- 需要这个改动作为**所有项目**的默认行为（而非单项目定制）

**哪些改动只需改 pipeline.yml？**
- 修改 shell 命令的 exit 判定逻辑（加/减/改 `exit 1` 条件）
- 修改 exception_key → CallbackCommand 的映射（把 `auto_fix` 换成 `abort`、`fail_and_skip` 等）
- 修改 max_retries / context_paths
- 复用已有 exception_key（但要新启用一个 pipeline.yml 里没配的 key，前提是 Rust 的 `match_exception`
  已经能产出这个 key）

---

## 系统角色与职责

> **Harness 视角：** 下表是一份典型的 **三层 harness 拓扑**——执行层（sandboxed executor）、
> 控制层（deterministic orchestrator）、推理层（LLM reasoner）。每一层只暴露必要的 I/O，
> 每一层都可独立替换（换 podman / 换调度器 / 换模型）而不破坏契约。

| 角色 | 载体 | 行动依据 | 职责 | 不做什么 |
|------|------|---------|------|---------|
| **Docker 容器** | `bollard` 拉起的一次性容器 | `pipeline.yml` 里 `steps[*].commands` 定义的 shell 命令 | 执行 step 命令，产出 stdout / stderr / exit code；所有构建、测试、扫描动作都在这里发生 | 不做任何决策；只负责"跑完并返回退出码" |
| **pipelight (Rust)** | 主进程，含 executor / scheduler / StepDef | `pipeline.yml` 的 `on_failure` 配置 + Rust 里 `match_exception` 的 stderr 匹配规则 | 根据 stderr 匹配 exception_key，去 pipeline.yml 查 CallbackCommand，决定 retry/skip/abort，汇总 pipeline status，输出 JSON | 不改代码、不调 LLM、不访问外部服务 |
| **LLM (Claude)** | 通过 pipelight-run skill 驱动 | JSON 里 `on_failure.command` + skill 的「回调命令处理表」 | 按表执行具体动作：读 stderr/context_paths、改源码、搜规范、生成配置、打印 pong、调 `pipelight retry` | 不决定"该不该重试"、"该不该终止"——这是 pipelight 的职责 |

**一句话划界：**
- Docker 容器回答"命令跑完了没 / 退出码是多少"（**exit 码规则写在 pipeline.yml**）
- pipelight 回答"失败了该干什么（retry/skip/abort）"（**决策表也写在 pipeline.yml**，Rust 只负责 stderr → exception_key 的翻译）
- LLM 回答"按指令具体怎么干"（动作定义在 skill）

---

## 完整控制链路

```
┌───────────────────────────────────────────────────────────────────┐
│ [Docker 容器]                                                      │
│  执行 pipeline.yml 的 steps[*].commands 定义的 shell 命令          │
│  产出 stdout / stderr / exit_code                                  │
│  退出码规则由 pipeline.yml 里的 shell 脚本定义                      │
│  （Rust 的 pmd_step.rs::format! 只是生成 pipeline.yml 的模板）      │
└───────────────────────────────────────────────────────────────────┘
                              ↓ exit_code + 输出流
┌───────────────────────────────────────────────────────────────────┐
│ [pipelight: executor 层]  src/ci/executor/                        │
│  捕获 container exit_code / stdout / stderr                        │
│  落盘 log 到 pipelight-misc/<step>-<timestamp>.log                │
└───────────────────────────────────────────────────────────────────┘
                              ↓
┌───────────────────────────────────────────────────────────────────┐
│ [pipelight: StepDef::match_exception(exit_code, stdout, stderr)]  │
│  src/ci/pipeline_builder/<lang>/<step>_step.rs                    │
│  用 stderr 里的特征字符串匹配 exception_key                        │
│                                                                    │
│  示例（gradle/pmd_step.rs）：                                      │
│    stderr 含 "Cannot load ruleset"   → "ruleset_invalid"           │
│    stderr 含 "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset"             │
│                                       → "ruleset_not_found"       │
│    其他                                → None (走默认)             │
└───────────────────────────────────────────────────────────────────┘
                              ↓ exception_key
┌───────────────────────────────────────────────────────────────────┐
│ [pipelight: 查 pipeline.yml 的 on_failure.exceptions 表]          │
│  用 exception_key 在 pipeline.yml 里查：                           │
│    steps[*].on_failure.exceptions.<key> = {                       │
│      command, max_retries, context_paths }                        │
│  查不到 → 用 steps[*].on_failure.callback_command 作为默认         │
│                                                                    │
│  （注：pipelight init 时这张表由 Rust 的                           │
│   StepDef::exception_mapping() 生成并烘焙进 pipeline.yml，         │
│   运行时只读 pipeline.yml，不读 Rust 代码）                        │
└───────────────────────────────────────────────────────────────────┘
                              ↓ CallbackCommand
┌───────────────────────────────────────────────────────────────────┐
│ [pipelight: CallbackCommandRegistry::action_for()]                │
│  src/ci/callback/command.rs                                        │
│  把 CallbackCommand → CallbackCommandAction                        │
│    Retry / Skip / Abort / RuntimeError                            │
└───────────────────────────────────────────────────────────────────┘
                              ↓ action
┌───────────────────────────────────────────────────────────────────┐
│ [pipelight: scheduler]  src/ci/scheduler/                         │
│  根据每个 step 的 action 汇总 pipeline 整体 status：                │
│    含 Retry 且 retries_remaining > 0 → "retryable"                │
│    只有 Skip/Success                 → "success"                  │
│    含 Abort/RuntimeError/retries=0    → "failed"                  │
│  输出 JSON (on_failure.command + .action + .retries_remaining)    │
└───────────────────────────────────────────────────────────────────┘
                              ↓ JSON
┌───────────────────────────────────────────────────────────────────┐
│ [LLM (Claude) + pipelight-run skill]                              │
│  读 on_failure.command，在 skill 的「回调命令处理表」里查对应动作    │
│  执行动作：打印 pong / 改源码 / 生成 ruleset / 报告失败              │
│  必要时调 `pipelight retry --run-id … --step …` 回到第一步         │
└───────────────────────────────────────────────────────────────────┘
```

---

## 实战演练：PMD ruleset 缺失 → LLM 搜索规范 → 生成 ruleset → retry 跑通

下面是一次真实执行的 pmd_step 全流程，取自 rc 项目（Gradle/Java 多模块项目）。这个例子把
"容器执行 → pipelight 决策 → LLM 修复 → retry"四个环节首尾相连地走完一遍，展示三方之间
实际流动的 JSON 与动作。

### 第 0 步：初次运行

LLM 侧调用（由 pipelight-run skill 触发）：

```bash
cd /Users/xiaojin/workspace/rc && \
pipelight run -f pipeline.yml --output json --run-id run-001
```

容器执行 `steps[pmd].commands` 里的 shell 脚本，检查 `/workspace/pipelight-misc/pmd-ruleset.xml`
不存在 → 走"没有 ruleset"分支：

```bash
echo 'PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. \
LLM should search project for existing ruleset or coding guidelines to generate one. \
IMPORTANT: Use PMD 7.9.0 rule names (not PMD 6.x). \
Verify rule names exist in PMD 7 before writing the ruleset.' >&2 && exit 1
```

### 第 1 步：pipelight 输出首次 JSON（retryable）

容器 exit_code = 1 → executor 落盘日志 → `match_exception` 匹配 stderr 命中
`ruleset_not_found` key → 查 pipeline.yml 的 `on_failure.exceptions.ruleset_not_found` →
拿到 `CallbackCommand::AutoGenPmdRuleset` + `max_retries: 2` → scheduler 汇总为 `retryable`：

```json
{
  "run_id": "run-001",
  "pipeline": "gradle-java-ci",
  "status": "retryable",
  "steps": [
    { "name": "git-pull", "status": "skipped",
      "on_failure": { "command": "git_fail", "action": "skip" } },
    { "name": "build", "status": "success",
      "report_summary": "Build succeeded" },
    {
      "name": "pmd",
      "status": "failed",
      "exit_code": 1,
      "image": "gradle:8-jdk17",
      "stderr": "PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset - No pmd-ruleset.xml found in pipelight-misc/. LLM should search project for existing ruleset or coding guidelines to generate one. IMPORTANT: Use PMD 7.9.0 rule names (not PMD 6.x). Verify rule names exist in PMD 7 before writing the ruleset.",
      "on_failure": {
        "exception_key": "ruleset_not_found",
        "command": "auto_gen_pmd_ruleset",
        "action": "retry",
        "max_retries": 2,
        "retries_remaining": 2,
        "context_paths": [
          "src/main/java/",
          "src/main/resources/",
          "build.gradle.kts"
        ]
      }
    },
    { "name": "test", "status": "skipped" }
  ]
}
```

### 第 2 步：LLM 按 skill 的「回调命令处理表」执行 `auto_gen_pmd_ruleset`

LLM 读到 `on_failure.command = "auto_gen_pmd_ruleset"`，在 skill 里查到对应流程（
两轮搜索：先找已有 ruleset，再找编码规范文档）。

**Round 1 — 搜已有 PMD 配置：**

```
Glob:  /Users/xiaojin/workspace/rc/**/pmd*.xml
Glob:  /Users/xiaojin/workspace/rc/**/pmd-ruleset*
Glob:  /Users/xiaojin/workspace/rc/**/config/pmd/**
Grep:  "pmd" in build.gradle*
→ 全部无结果
```

**Round 2 — 搜编码规范文档：**

```bash
cd /Users/xiaojin/workspace/rc && \
find . -maxdepth 3 -type f \
  \( -iname "*规范*" -o -iname "*guideline*" -o -iname "*coding*" \)
```

命中：

```
./docs/kt后端编码规范.md
./docs/阿里巴巴Java开发规范（嵩山版）.pdf
```

LLM 读取 `docs/kt后端编码规范.md` 的全部内容，把里面的具体条款（命名规范、禁止 try-catch
进 controller、禁止魔法值、禁止 SQL 字符串拼接、复杂度控制……）映射到 PMD 7.9.0 的规则名，
并叠加阿里巴巴 Java 开发规范的常见规则，写出新文件：

**LLM 的 Write 动作：**

```
Write /Users/xiaojin/workspace/rc/pipelight-misc/pmd-ruleset.xml
```

文件片段（约 100 行，只截首尾）：

```xml
<?xml version="1.0"?>
<ruleset name="rc-java-ruleset"
         xmlns="http://pmd.sourceforge.net/ruleset/2.0.0">
    <description>
        RC project PMD ruleset (PMD 7.9.0). Derived from the project's
        Kotlin backend coding guideline (docs/kt后端编码规范.md) and the
        Alibaba Java Development Manual (嵩山版).
    </description>

    <!-- Naming -->
    <rule ref="category/java/codestyle.xml/ClassNamingConventions"/>
    <rule ref="category/java/codestyle.xml/MethodNamingConventions"/>
    <rule ref="category/java/codestyle.xml/FieldNamingConventions"/>
    ...
    <!-- Error prone -->
    <rule ref="category/java/errorprone.xml/EmptyCatchBlock"/>
    <rule ref="category/java/errorprone.xml/AvoidDuplicateLiterals">
        <properties>
            <property name="maxDuplicateLiterals" value="4"/>
        </properties>
    </rule>
    ...
</ruleset>
```

### 第 3 步：LLM 触发 retry

```bash
cd /Users/xiaojin/workspace/rc && \
pipelight retry --run-id run-001 --step pmd \
  -f pipeline.yml --output json
```

**注意这里 harness 工程的三个细节：**
- 必须带**同一个** `--run-id run-001`，harness 用它把两次调用关联到同一次执行上下文
- 必须指定 `--step pmd`，否则 harness 不知道要重跑哪一步
- 输出依然是 JSON，便于 LLM 再次解析（LLM ↔ harness 从不说自由文本）

### 第 4 步：pipelight 输出 retry 后的 JSON

同一个容器 image `gradle:8-jdk17` 重新拉起，这次 `/workspace/pipelight-misc/pmd-ruleset.xml`
已存在 → 走"有 ruleset"分支 → 尝试 Gradle PMD 插件（不存在）→ 回落到 standalone PMD CLI →
扫描所有 `src/main/java` → 生成 `pmd-report/pmd-result.xml` + `pmd-summary.txt` →
命令 `exit 0`（当前行为：违规不 fail）：

```json
{
  "run_id": "run-001",
  "pipeline": "gradle-java-ci",
  "status": "failed",
  "steps": [
    { "name": "build", "status": "success", "report_summary": "Build succeeded" },
    {
      "name": "pmd",
      "status": "success",
      "exit_code": 0,
      "duration_ms": 228000,
      "stdout": "...\nPMD Total: 1604 violations\n\n=== Violations by Rule ===\n    603 UnnecessaryImport\n    374 FieldNamingConventions\n     95 ExcessiveImports\n     ...",
      "report_summary": "PMD Total: 1604 violations",
      "report_path": "pipelight-misc/pmd-report/pmd-summary.txt",
      "on_failure": null
    },
    {
      "name": "test",
      "status": "failed",
      "stderr": "Execution failed for task ':base:rc-utils:test'. ...",
      "on_failure": { "command": "abort", "action": "abort" }
    }
  ]
}
```

### 第 5 步：LLM 生成最终汇报

LLM 解析 JSON，发现 pmd step 已 success（虽然 1604 violations 但当前 shell 逻辑不让它 fail），
剩下的 test step 是 `abort` 类，不 retry，直接给用户出收尾报告：

```markdown
## Pipeline Failed

| Step | Status | Summary |
|------|--------|---------|
| git-pull | skipped | git_fail (network) |
| build | success | Build succeeded |
| pmd | success | PMD Total: 1604 violations |
| test | failed | JUnit Platform missing TestEngine |

**Auto-fix History (1 round):**
- `pipelight-misc/pmd-ruleset.xml` — generated from
  `docs/kt后端编码规范.md` + 阿里巴巴 Java 开发规范（PMD 7.9.0）
```

### 这一次 harness 交互干了什么

| 阶段 | 谁在行动 | 产出 |
|------|---------|------|
| 容器初跑 | Docker 容器 | `exit 1` + `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset` |
| 首次 JSON | pipelight (Rust) | `status=retryable`, `command=auto_gen_pmd_ruleset`, `retries_remaining=2` |
| 搜索 + 生成 | LLM + skill | `pipelight-misc/pmd-ruleset.xml`（从编码规范文档推导） |
| Retry 触发 | LLM | `pipelight retry --run-id run-001 --step pmd` |
| 容器重跑 | Docker 容器 | `exit 0` + `PMD Total: 1604` |
| 终态 JSON | pipelight (Rust) | `steps[pmd].status=success`，`test` step 接手失败 |

**三个值得注意的 harness 行为：**
1. **pipelight 从未告诉 LLM "去搜 docs 目录"**——它只发出抽象指令 `auto_gen_pmd_ruleset`，
   具体怎么搜、搜哪些路径、读哪个文件，全部由 skill 的「`auto_gen_pmd_ruleset` 详细流程」
   小节预先定义。harness 的指令是**声明式**的（"要什么"），不是**过程式**的（"怎么做"）
2. **`retries_remaining=2` 是硬上限**——即便 LLM 搜了一轮没结果，它也只剩 1 次额外尝试。
   如果第二轮也找不到编码规范，skill 规定 LLM 必须**新起 run** 并 `--skip pmd`，而不是无限
   重试。这就是 harness 的"有界自治"
3. **run_id `run-001` 贯穿全程**——pipelight 用它把"初跑 + retry"绑定成同一次执行上下文，
   既保证幂等（已成功的 step 不会重跑），也方便事后回放（`pipelight-misc/` 下所有日志都带
   同一 run_id 时间戳）

---

## CallbackCommand：harness 的"指令集架构"

`CallbackCommand` 枚举是这个 harness 的 **ISA（instruction set architecture）**——
pipelight 能向 LLM 发出的指令只有这几种，LLM 的行为也只能是这几种的组合。用枚举
而非自由文本作为接口，天然具备三个 harness 工程特征：

- **可穷举**：所有 LLM 可能做的动作都被 skill 的处理表显式枚举，没有"意外行为"
- **可审计**：一次 pipeline 的所有决策都可以用一串 CallbackCommand 复述出来
- **可扩展**：新增一种失败响应策略 = 新增一个枚举值 + skill 里一行处理规则



`CallbackCommand` 枚举定义在 `src/ci/callback/command.rs`。每个枚举值既绑定一个
`CallbackCommandAction`（pipelight 侧汇总 pipeline status 的依据），也对应 skill 中 LLM 的一段动作脚本：

| `CallbackCommand` | Action (pipelight) | pipeline status | LLM 收到后做什么 | 典型场景 |
|-------------------|-------------------|-----------------|------------------|----------|
| `AutoFix` | Retry | `retryable` | 读 stderr + context_paths → 修源码 → `pipelight retry` | 编译错误、测试失败 |
| `AutoGenPmdRuleset` | Retry | `retryable` | 搜索已有 ruleset 或从编码规范生成 → retry；都找不到则 `--skip pmd` 新起 run | PMD ruleset 缺失 |
| `Ping` | Retry | `retryable` | 打印 `pong` → retry（10 轮后 step 自动成功） | ping-pong 通信测试 |
| `GitFail` | Skip | （该 step skip，pipeline 继续） | 无操作 | git 网络/认证失败 |
| `FailAndSkip` | Skip | （该 step skip，pipeline 继续） | 无操作 | 前置条件缺失 |
| `RuntimeError` | RuntimeError | `failed` | 报告错误，不 retry、不继续 | 运行时错误 |
| `Abort` | Abort | `failed` | 报告错误，不 retry、不继续 | 严重代码问题 |

三类核心行为，对应 harness 中经典的 **三族失败响应策略**：
- **retry 类**（`AutoFix` / `AutoGenPmdRuleset` / `Ping`）→ 有界自治修复，`max_retries` 即熔断器
- **skip 类**（`GitFail` / `FailAndSkip`）→ 降级继续，harness 自主决策，LLM 完全不介入
- **终止类**（`RuntimeError` / `Abort`）→ fail-fast，保护下游不受污染数据影响

---

## 案例：把 PMD 违规从"success"改成"auto_fix"

> **Harness 视角：** 这是一个"策略变更而非代码变更"的典型案例。在传统 CI 里，要把 PMD
> 的处理从"仅报告"改成"违规即让机器人修复"，往往意味着改 Jenkinsfile + 新脚本 + 新插件；
> 在本 harness 里，这是一次**一行 YAML + 一段 shell 片段**的配置变更，LLM 的行为空间早就
> 被预定义好了，无需新增任何 LLM 提示词。


**当前行为：** PMD 扫描出 1604 个违规，但容器 `exit 0` → pipelight 判 step = success → pipeline 继续。

**目标行为：** PMD 违规 > 0 时，pipeline 进入 `retryable`，LLM 读报告逐步修复源码后重试。

这个改动有两条路径——**方式 A（只改 pipeline.yml，项目级定制）** 和
**方式 B（改 Rust 模板，所有项目默认生效）**。先看方式 A，因为大多数场景都应该走 A。

---

### 方式 A：只改 pipeline.yml（推荐，项目级定制）

直接编辑项目根目录的 `pipeline.yml`，无需重编译 pipelight，无需改 LLM/skill。

#### 三角色改动全景（方式 A）

| 角色 | 改什么 | 改在哪 | 只改 pipeline.yml 够不够 |
|------|--------|--------|--------------------------|
| **Docker 容器** | shell 命令——违规数 > 0 时 `exit 1` + 打印 `PIPELIGHT_CALLBACK:auto_fix` | `pipeline.yml` 的 `steps[pmd].commands` | ✅ 够 |
| **pipelight** | exception_key → CallbackCommand 映射 | `pipeline.yml` 的 `steps[pmd].on_failure` | ⚠️ 部分够——见下方"约束" |
| **LLM / skill** | 无改动 | — | ✅ 完全不用动 |

#### 改动 1：`steps[pmd].commands`——加违规时退出逻辑

在现有 shell 命令末尾（生成 `pmd-summary.txt` 之后、最外层 `fi` 之前）拼接：

```bash
echo "PMD Total: $TOTAL violations";
if [ "$TOTAL" -gt 0 ]; then
  echo "PIPELIGHT_CALLBACK:auto_fix - PMD found $TOTAL violations. \
See /workspace/pipelight-misc/pmd-report/pmd-summary.txt for rule/file breakdown, \
/workspace/pipelight-misc/pmd-report/pmd-result.xml for per-location details." >&2
  exit 1
fi
```

这段只改变容器的退出码约定。容器本身没有决策逻辑——它只是 `exit 1`，把"怎么解读"交给 pipelight。

#### 改动 2：`steps[pmd].on_failure`——换默认 callback

这里有一个关键约束：**"stderr 特征串 → exception_key"的翻译逻辑写在 Rust 里**
（`pmd_step.rs::match_exception`），`pipeline.yml` 没有地方配它。当前 `match_exception` 只能产出：
- `ruleset_invalid`（stderr 含 `Cannot load ruleset` / `Unable to find referenced rule`）
- `ruleset_not_found`（stderr 含 `PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset`）
- `None`（其他所有情况，走默认 `on_failure.callback_command`）

新的 `PIPELIGHT_CALLBACK:auto_fix` stderr 不在 Rust 匹配表里 → 会落到 `None` → 走**默认** callback。
所以方式 A 的做法是**直接改默认 callback**：

```yaml
steps:
- name: pmd
  on_failure:
    callback_command: auto_fix        # ← 从 runtime_error 改成 auto_fix
    max_retries: 3                    # ← 从 2 改成 3
    context_paths:                    # ← 新增 PMD 报告路径
    - pipelight-misc/pmd-report/pmd-summary.txt
    - pipelight-misc/pmd-report/pmd-result.xml
    - src/main/java/
    - src/main/resources/
    exceptions:
      ruleset_not_found: { ... }      # 保留原样
      ruleset_invalid:   { ... }      # 保留原样
```

运行时行为：
- PMD ruleset 缺失 → stderr 命中 `ruleset_not_found` → 走 `auto_gen_pmd_ruleset`（原流程不变）
- PMD 规则名无效 → stderr 命中 `ruleset_invalid` → 走 `auto_gen_pmd_ruleset`（原流程不变）
- **PMD 有违规** → stderr 不命中任何 key → 走默认 `auto_fix` ✓

#### 方式 A 完整改动清单（pipeline.yml only）

| 字段 | 改动 |
|------|------|
| `steps[pmd].commands` | 末尾加 `if [ "$TOTAL" -gt 0 ]; then echo PIPELIGHT_CALLBACK:auto_fix ... >&2; exit 1; fi` |
| `steps[pmd].on_failure.callback_command` | `runtime_error` → `auto_fix` |
| `steps[pmd].on_failure.max_retries` | `2` → `3` |
| `steps[pmd].on_failure.context_paths` | 加入 `pipelight-misc/pmd-report/pmd-summary.txt` 和 `pmd-result.xml` |

改完直接 `pipelight run`，无需重编译。**注意**：之后如果运行 `pipelight init --reinit`
会用 Rust 模板覆盖这些手工改动。

#### 切换到其他策略（pipeline.yml 一行改动）

只改 `on_failure.callback_command` 一个字段：

| 目标行为 | `callback_command` 值 | LLM 行动 |
|---------|----------------------|---------|
| 让 LLM 修复 | `auto_fix` | retry + 改源码 |
| 直接终止 pipeline | `abort` | 只报告，不 retry |
| 跳过 pmd 继续 | `fail_and_skip` | 无操作，pipelight 自动 skip |

**这就是 pipeline.yml 配置化的价值：** shell 命令和 LLM 行为都不用动，改一行 YAML 即可切换策略。

---

### 方式 B：改 Rust 模板（影响所有 `pipelight init` 生成的项目）

当改动应作为**所有项目**的默认行为、或者需要精细区分 exception_key 时，才走方式 B。

#### 三角色改动全景（方式 B）

| 角色 | 改什么 | 改在哪 |
|------|--------|--------|
| **Docker 容器** | shell 命令模板 | `src/ci/pipeline_builder/gradle/pmd_step.rs`、`maven/pmd_step.rs` 的 `StepDef::config()` 里 `format!` 宏 |
| **pipelight** | stderr → exception_key 匹配规则 | 同上两文件的 `StepDef::match_exception()` |
| **pipelight** | exception_key → CallbackCommand 映射模板 | 同上两文件的 `StepDef::exception_mapping()` |
| **LLM / skill** | 基本不用动 | — |

#### 改动点 A：shell 命令模板（同方式 A 改动 1）

在 `format!` 宏里生成 `pmd-summary.txt` 之后、最外层 `else` 分支之前插入相同的 `if [ "$TOTAL" -gt 0 ]` 片段。

#### 改动点 B：`match_exception` 新增 stderr 分支

```rust
fn match_exception(&self, _exit_code: i64, _stdout: &str, stderr: &str) -> Option<String> {
    if stderr.contains("Cannot load ruleset")
        || stderr.contains("Unable to find referenced rule")
    {
        Some("ruleset_invalid".into())
    } else if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_pmd_ruleset") {
        Some("ruleset_not_found".into())
    } else if stderr.contains("PIPELIGHT_CALLBACK:auto_fix") {   // ← 新增
        Some("violations_found".into())
    } else {
        None
    }
}
```

#### 改动点 C：`exception_mapping` 新增映射

```rust
.add("violations_found", ExceptionEntry {
    command: CallbackCommand::AutoFix,
    max_retries: 3,
    context_paths: {
        let mut p = self.source_paths.clone();
        p.push("pipelight-misc/pmd-report/pmd-summary.txt".into());
        p.push("pipelight-misc/pmd-report/pmd-result.xml".into());
        p
    },
})
```

#### 无需改动（方式 B）

- `src/ci/callback/command.rs` —— `CallbackCommand::AutoFix` 已存在，已绑定 `Retry` action
- `src/ci/executor/` —— 只负责捕获 exit code，对特定 step 无感知
- `src/ci/scheduler/` —— 按 action 汇总 pipeline status 的逻辑通用，不需针对 PMD 特化
- `global-skills/pipelight-run/SKILL.md` —— `auto_fix` 已在处理表中（可选补"遇 pmd-summary 分批修"的注记）

#### 方式 B 生效流程

1. 改完 Rust 代码 → `cargo build`
2. 在项目中运行 `pipelight init --reinit`（或先删 `pipeline.yml` 再 `pipelight init`）
3. 新生成的 `pipeline.yml` 里会同时包含新 shell 命令和新 `violations_found` 映射
4. 运行 `pipelight run`

---

### 方式 A vs 方式 B 如何选？

| 场景 | 推荐方式 |
|------|----------|
| 只对当前项目调整 PMD 行为 | **方式 A**（pipeline.yml） |
| 快速调试、验证策略 | **方式 A** |
| 作为所有 Gradle/Maven 项目的默认行为 | **方式 B** |
| 需要区分多个 stderr 特征为不同 exception_key | **方式 B**（必须） |
| 需要新增 `CallbackCommand` 枚举值 | **方式 B**（必须，且要同步 skill） |

---

## 新增 CallbackCommand 的同步规则

当在 `src/ci/callback/command.rs` 新增枚举值时，**必须同步更新**：

1. `CallbackCommandRegistry::new()` 里注册新 command 对应的 `CallbackCommandAction` 和描述
2. `global-skills/pipelight-run/SKILL.md` 的「回调命令处理表」新增一行
3. 如果表格一句话说不清 LLM 操作，在表格下方新增 **`<command_name>` 详细流程** 小节
4. 执行 `cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/` 同步到本地

未同步会导致 LLM 收到新命令后"看不懂该做什么"——LLM 的行为表完全来自 skill。

> **Harness 视角：** 这条同步规则保护的是"指令集架构"的完整性——ISA 扩展必须同时更新
> 控制平面的解码器（`CallbackCommandRegistry`）和执行平面的微码（skill 处理表）。漏掉
> 任一侧都会出现"指令已发出但无人能解读"的幽灵状态，这在任何 harness 里都是最难排查的
> 一类 bug。

---

## 设计回顾：这个 harness 为什么可靠

把前面章节串起来看，pipelight × Claude Code × skill 这套组合之所以敢把 LLM 放进 CI/CD
流水线里跑生产任务，靠的不是"LLM 足够聪明"，而是以下几条 harness 工程约束：

1. **单向控制流**：pipelight 向 LLM 下发指令，LLM 只能通过 `pipelight retry` 回调；
   LLM 不能反向驱动 pipelight 状态机，也不能修改 harness 内部数据结构
2. **枚举而非自由文本**：所有决策点都用 `CallbackCommand` 枚举，杜绝"让 LLM 判断要不要继续"
   这种把控制权让渡出去的反模式
3. **预算即熔断**：`max_retries` 把每次 LLM 自治修复限制在有限成本内，配额耗尽自动 fail-close
4. **结构化 I/O**：JSON 输出 + 明确 schema，既方便 LLM 解析，也方便人类审计与日志采集
5. **配置即契约**：`pipeline.yml` 让整个 harness 的行为对项目开发者透明，不需读 Rust 也能调优
6. **可重放性**：`run_id` + 落盘日志让每一次 pipeline 都可以在事后精确复盘
7. **最小权限上下文**：`context_paths` 显式列出 LLM 允许读取的文件，降低越权与幻觉成本
8. **职责隔离即可替换性**：容器 / 调度器 / 模型任何一层都能独立升级，契约不变行为就不变

这些约束没有一条是"LLM 特有"的，它们都是几十年来分布式系统、作业调度、RPC 框架沉淀下来的
工程智慧。**LLM harness 的本质，就是把这些经典手艺应用到"有一个会写代码的非确定性组件"这种
新场景上。**

---

## 互动流程状态机

前面几章分别讲了"谁是谁"（角色）、"谁调谁"（控制链路）、"怎么重试"（回调命令）。
这里把整个 pipelight ↔ LLM 协作过程**压缩成一张状态机**，作为全局视角的快速索引。

### 状态节点

| 状态 | 归属方 | 含义 |
|------|--------|------|
| `StepPending` | pipelight | step 已排入 DAG，等待前置依赖完成 |
| `StepRunning` | pipelight | 容器已启动，命令执行中 |
| `StepExited` | pipelight | 容器退出，`exit_code` 捕获完毕 |
| `StepSuccess` | pipelight | `exit_code == 0`，无需回调 |
| `StepFailed` | pipelight | `exit_code != 0`，进入回调解析 |
| `Resolved` | pipelight | `ExceptionMapping::resolve` 产出 `(command, action, retries_remaining)` |
| `AwaitingLLM` | pipelight | JSON 已输出含 `on_failure`，进程退出或等待 `retry` 调用 |
| `LLMActing` | Claude Code + skill | skill 按 `command` 执行对应 LLM 操作（读 context_paths / 改源码 / 生成配置） |
| `LLMRetry` | Claude Code | LLM 调用 `pipelight retry --step <name>`，`retries_remaining -= 1` |
| `StepSkipped` | pipelight | `action == Skip` 或 `FailAndSkip` / `GitFail` 触发，自动跳过 |
| `PipelineAbort` | pipelight | `action == Abort` 或 `RuntimeError`，整条流水线终止 |
| `RetriesExhausted` | pipelight | `retries_remaining == 0` 且仍失败，fail-close 为 `PipelineAbort` |
| `PipelineDone` | pipelight | 所有 step 处于 `StepSuccess` 或 `StepSkipped`，流水线结束 |

### 状态转移图

```
                     ┌──────────────┐
                     │ StepPending  │
                     └──────┬───────┘
                            │ 依赖就绪
                            ▼
                     ┌──────────────┐
                     │ StepRunning  │
                     └──────┬───────┘
                            │ 容器退出
                            ▼
                     ┌──────────────┐
                     │  StepExited  │
                     └──┬────────┬──┘
                 exit==0│        │exit!=0
                        ▼        ▼
               ┌────────────┐  ┌────────────┐
               │ StepSuccess│  │ StepFailed │
               └──────┬─────┘  └──────┬─────┘
                      │               │ ExceptionMapping::resolve
                      │               ▼
                      │        ┌────────────┐
                      │        │  Resolved  │
                      │        └──────┬─────┘
                      │   ┌───────────┼───────────┬──────────────┐
                      │   │action=    │action=    │action=       │action=
                      │   │Retry      │Skip       │Abort         │RuntimeError
                      │   ▼           ▼           ▼              ▼
                      │ ┌─────────┐ ┌─────────┐ ┌──────────────┐
                      │ │Awaiting │ │  Step   │ │PipelineAbort │
                      │ │  LLM    │ │ Skipped │ └──────────────┘
                      │ └────┬────┘ └────┬────┘
                      │      │ skill 解析 on_failure.command
                      │      ▼
                      │ ┌──────────┐
                      │ │LLMActing │  (按 command 执行修复/生成/跳过判断)
                      │ └────┬─────┘
                      │      │ pipelight retry --step <name>
                      │      ▼
                      │ ┌──────────┐     retries_remaining>0
                      │ │ LLMRetry ├────────────────┐
                      │ └────┬─────┘                │
                      │      │ retries_remaining==0 │
                      │      ▼                      │
                      │ ┌──────────────┐            │
                      │ │RetriesExhaust│            │
                      │ └──────┬───────┘            │
                      │        │                    │
                      │        ▼                    │
                      │ ┌──────────────┐            │
                      │ │PipelineAbort │            │
                      │ └──────────────┘            │
                      │                             ▼
                      │                      (回到 StepRunning，重跑该 step)
                      │                             │
                      └─────────┬───────────────────┘
                                │ DAG 所有 step 终态
                                ▼
                        ┌──────────────┐
                        │ PipelineDone │
                        └──────────────┘
```

### 关键转移的代码位点

| 转移 | 触发代码 |
|------|----------|
| `StepExited → StepSuccess / StepFailed` | `src/cli/mod.rs` 判断 `result.success` |
| `StepFailed → Resolved` | `ExceptionMapping::resolve` (`src/ci/callback/exception.rs`) |
| `Resolved → (Retry/Skip/Abort/RuntimeError)` | `CallbackCommandRegistry::action_for` (`src/ci/callback/command.rs` + `action.rs`) |
| `Resolved → AwaitingLLM` | 输出 JSON 含 `on_failure`，pipelight 进程退出 |
| `AwaitingLLM → LLMActing` | `pipelight-run` skill 读取 JSON，按"回调命令处理表"分派 |
| `LLMActing → LLMRetry` | skill 末尾执行 `pipelight retry --step <name>` |
| `LLMRetry → StepRunning` | `retry` 子命令重入执行器，`retries_remaining -= 1` |
| `LLMRetry → RetriesExhausted` | `retries_remaining == 0` 时 fail-close |

### 三条不可违反的硬约束

状态机能闭环，依赖这三条硬约束，任何一条被打破都会导致"LLM 看似被挂了回调，但实际不生效"：

1. **一级信号先行**：只有 `StepExited` 转移到 `StepFailed`（即容器非 0 退出）时，`ExceptionMapping::resolve` 才会被调用。`exit 0` 直接短路到 `StepSuccess`，无视所有 `exception_mapping` / `match_exception` 配置。
2. **预算即熔断**：`retries_remaining` 单调递减，LLM 无法自行续命。配额耗尽必然进入 `PipelineAbort`，不存在"LLM 判断要不要再试一次"这种反模式。
3. **单向控制流**：LLM 只能通过 `pipelight retry` 回到 `StepRunning`，不能直接跳到 `PipelineDone`、`StepSkipped` 或修改 `retries_remaining`。harness 状态永远由 pipelight 持有。

---

## Q&A

### Q: 我把某个 step 的 `exception_mapping` 配成了 `AutoFix`，为什么 LLM 没有自动修改代码？

**先检查这一步是不是真的"失败"了。**

这是一个高频踩坑点：`on_failure_state`（也就是回调链路的入口）**只在 `result.success == false` 时才构建**，见 `src/cli/mod.rs`：

```rust
let mut on_failure_state = if !result.success {
    // 这里才会调用 exception_mapping + match_exception + registry.action_for
    ...
};
```

也就是说，只要容器退出码是 0，pipelight 就把这一步判定为成功：
- 不调用 `match_exception`
- 不 resolve `ExceptionMapping`
- 不下发任何 `CallbackCommand`
- JSON 输出里 `on_failure` 字段直接为空

此时哪怕你在 `exception_mapping()` 里把 default 写成 `AutoFix`、`match_exception()` 里无条件返回 `Some("xxx")`，**都是死代码**，LLM 端完全看不到任何回调信号，skill 自然不会执行自动修复。

**真实案例**：早期的 `maven/spotbugs_step.rs` 为了"只出报告不阻断"，shell 脚本末尾写了 `exit 0`。结果 SpotBugs 扫出 800+ 个 bug，pipelight 仍然把它标成 ✓ 成功，`AutoFix` 永远不触发。对比 `maven/pmd_step.rs`，它在发现违规时显式 `exit 1`，所以 PMD 的 `AutoFix` 才能正常工作。

**判定准则（写 step 时务必遵守）**：

> **一个 step 想要触发任何 `CallbackCommand` 回调（`AutoFix` / `AutoGenPmdRuleset` / `FailAndSkip` / `GitFail` / …），它的容器命令就必须在目标情况下以非 0 退出码结束。`exit 0` 等于告诉 pipelight"这一步完全成功、无需任何后续动作"，和挂一个回调的意图是互相矛盾的。**

换句话说：**退出码是 harness 的一级信号，`exception_mapping` 只是二级解析**。一级信号没响，二级永远不会被读取。

如果你确实想要"出报告但不阻断 pipeline"，那就不要挂 `AutoFix`，而应该：
- 要么让该 step 真的 `exit 0` 并且 `exception_mapping` 里不配任何会驱动 LLM 的 command，
- 要么让它 `exit 1` 并把 default command 设为 `FailAndSkip`（pipelight 会自动 skip 并继续后续 step，不打扰 LLM）。

**排查 checklist**（从现象倒查到根因，按顺序走）：

1. pipelight JSON 输出里这一步的 `success` 字段是 `true` 还是 `false`？`true` → 根因就是 exit 0，改 shell 脚本。
2. `on_failure` 字段为空？确认 1 之后就能解释。
3. `on_failure.command` 出现了但不是你期望的值？去看 `match_exception` 返回的 key 是否落在 `exception_mapping` 的 entries 里，没命中会 fallback 到 default。
4. command 正确但 LLM 端没反应？去 `global-skills/pipelight-run/SKILL.md` 的"回调命令处理表"确认该 command 的 action，以及 skill 是否加载。

---

## 相关代码位置速查

| 关注点 | 文件 |
|--------|------|
| CallbackCommand 枚举定义 + registry | `src/ci/callback/command.rs` |
| CallbackCommandAction 枚举（Retry/Skip/Abort/RuntimeError） | `src/ci/callback/action.rs` |
| ExceptionMapping / ExceptionEntry | `src/ci/callback/exception.rs` |
| StepDef trait（含 `match_exception` / `exception_mapping`） | `src/ci/pipeline_builder/base/mod.rs` |
| 各语言各 step 的具体实现 | `src/ci/pipeline_builder/<lang>/<step>_step.rs` |
| executor 层捕获 container exit code | `src/ci/executor/` |
| scheduler 按 action 汇总 pipeline status | `src/ci/scheduler/` |
| LLM 侧回调命令处理表 | `global-skills/pipelight-run/SKILL.md` |
