# JaCoCo 覆盖率 Step 设计稿

- **日期**：2026-04-20
- **作者**：xiaojin（brainstorming with Claude）
- **状态**：Draft → Awaiting Review

## 背景与目标

给 Maven 和 Gradle pipeline 加上**单元测试覆盖率检查**，不达标时走 skill 回调机制让 LLM 自动补单元测试。

**核心需求**：
1. 在 Maven 和 Gradle pipeline 中插入 JaCoCo step。
2. 覆盖率默认只统计 **git-diff 中的变更文件**（与现有 PMD/SpotBugs 增量扫描语义对齐）。
3. 默认阈值 **70%**，未达标时走 `AutoFix` 回调，LLM 为未覆盖路径补 UT，然后重试。
4. 维持现有项目风格：零侵入（不强制改项目 pom.xml / build.gradle），独立缓存，与 PMD/SpotBugs 架构对称。

## 关键决策（已与用户确认）

| # | 决策 | 选择 |
|---|---|---|
| 1 | step 数量 | **双胞胎模式**：`jacoco`（增量，hard-fail + AutoFix）+ `jacoco_full`（全仓，report-only + `JacocoPrintCommand`），镜像 PMD/SpotBugs |
| 2 | 执行机制 | **混合模式（Hybrid）**：detector 识别项目是否已配 JaCoCo 插件；有则走插件模式，无则走独立 agent CLI |
| 3 | 覆盖率语义 | **LINE 覆盖率，逐文件 ≥70%**（非全局聚合） |
| 4 | 排除规则 | **可配置**，默认排除 `*Dto.java`、`*Config.java`、`*Exception.java`、`*Application.java`；配置放 `pipelight-misc/jacoco-config.yml`，首次缺失触发 `AutoGenJacocoConfig` 回调 |
| 5 | 阈值可配 | yaml 顶层字段 `threshold: 70`（用户可改） |
| 6 | AutoFix 重试次数 | **9**（和 PMD 对齐） |
| 7 | DAG 位置 | Maven：`... → pmd_full → test → jacoco → jacoco_full → package`；Gradle：`... → jacoco → jacoco_full`（末尾） |
| 8 | test step 改造 | decorator `JacocoAgentTestStep`（解耦 jacoco-specific 逻辑，和 `MavenCachedStep` 同构） |
| 9 | XML 解析 | **shell inline**（grep/awk/sed），与 pmd_step 风格一致，零额外依赖 |
| 10 | 配置文件格式 | YAML |
| 11 | JaCoCo 版本 | **0.8.12**（支持 Java 5–22 字节码，CLI 只要 JRE 1.8+；兼容用户 Java 8/17 项目跨度） |
| 12 | exec 文件统一路径 | 两种模式都写到 `/workspace/pipelight-misc/jacoco-report/jacoco.exec` |

## 架构总览

```
现有 DAG (Maven):
  build → (checkstyle) → spotbugs → spotbugs_full → pmd → pmd_full → test → package

新 DAG (Maven):
  build → (checkstyle) → spotbugs → spotbugs_full → pmd → pmd_full
        → test(JacocoAgentTestStep 包裹) → jacoco → jacoco_full → package

现有 DAG (Gradle):
  build → (checkstyle) → spotbugs → spotbugs_full → pmd → pmd_full → test

新 DAG (Gradle):
  build → (checkstyle) → spotbugs → spotbugs_full → pmd → pmd_full
        → test(JacocoAgentTestStep 包裹) → jacoco → jacoco_full
```

新增/修改文件总览：

```
src/ci/detector/
  maven.rs                              [修改]  识别 jacoco-maven-plugin → quality_plugins
  gradle.rs                             [修改]  识别 jacoco 插件 (groovy + kts) → quality_plugins

src/ci/callback/command.rs              [修改]
  新增 enum 变体: AutoGenJacocoConfig, JacocoPrintCommand
  Registry 注册: AutoGenJacocoConfig→Retry, JacocoPrintCommand→JacocoPrint

src/ci/callback/action.rs               [修改]
  新增 enum 变体: JacocoPrint

src/ci/pipeline_builder/base/
  test_step.rs                          [修改]  新增 with_jacoco_agent 钩子 (由 decorator 使用)
  jacoco_agent_decorator.rs             [新增]  JacocoAgentTestStep (包装 TestStep 注入 agent/plugin)

src/ci/pipeline_builder/maven/
  jacoco_step.rs                        [新增]  MavenJacocoStep (增量)
  jacoco_full_step.rs                   [新增]  MavenJacocoFullStep (全仓)
  mod.rs                                [修改]  组装 + 调整 package.depends_on

src/ci/pipeline_builder/gradle/
  jacoco_step.rs                        [新增]  GradleJacocoStep (增量)
  jacoco_full_step.rs                   [新增]  GradleJacocoFullStep (全仓)
  mod.rs                                [修改]  组装末尾追加两个 step

global-skills/pipelight-run/SKILL.md    [修改]  在"回调命令处理表"新增 AutoGenJacocoConfig 和 JacocoPrintCommand 两行 + 对应详细章节
```

## 组件详述

### 1. Detector 扩展

**Maven** (`src/ci/detector/maven.rs`)：
- 扫描 `pom.xml`，发现字符串 `jacoco-maven-plugin` 时 push `"jacoco"` 到 `ProjectInfo.quality_plugins`

**Gradle** (`src/ci/detector/gradle.rs`)：
- 扫描 `build.gradle` / `build.gradle.kts`，以下任一模式匹配时 push `"jacoco"`：
  - `id 'jacoco'` / `id("jacoco")`
  - `apply plugin: 'jacoco'`

### 2. Callback 扩展

`src/ci/callback/command.rs` 新增：

```rust
pub enum CallbackCommand {
    // ... 既有 ...
    AutoGenJacocoConfig,
    JacocoPrintCommand,
}
```

Registry 注册：

| CallbackCommand | Action | 描述 |
|---|---|---|
| `AutoGenJacocoConfig` | `Retry` | LLM 结合项目特征生成 `pipelight-misc/jacoco-config.yml`（含 threshold + exclude 列表），然后 retry |
| `JacocoPrintCommand` | `JacocoPrint` | 全仓覆盖率扫描完成 → LLM 解析 XML 报告打印分组汇总表，pipeline 继续 |

`src/ci/callback/action.rs` 新增 `CallbackCommandAction::JacocoPrint` 变体。

### 3. JacocoAgentTestStep decorator

新文件 `src/ci/pipeline_builder/base/jacoco_agent_decorator.rs`：

```rust
pub enum JacocoMode {
    None,                                    // 无 JaCoCo (Rust/其他语言)
    Standalone { agent_jar_path: String },   // 下载 agent 注入 env
    MavenPlugin,                             // 走 jacoco:prepare-agent
    GradlePlugin,                            // 走 jacocoTestReport
}

pub struct JacocoAgentTestStep {
    inner: Box<dyn StepDef>,
    mode: JacocoMode,
}
```

`config()` 行为：
- **None**：原样返回 inner 的 config（回归保护）
- **Standalone**：
  - 在命令头前置：下载 `jacocoagent.jar` 到 `~/.pipelight/cache/jacoco-0.8.12/lib/`（若缺失）
  - 前置 `export MAVEN_OPTS="-javaagent:<agent.jar>=destfile=/workspace/pipelight-misc/jacoco-report/jacoco.exec,append=false"` 或 Gradle 用 `JAVA_TOOL_OPTIONS`
  - 原命令不动
- **MavenPlugin**：
  - 命令里的 `mvn test` → `mvn jacoco:prepare-agent test -Djacoco.destFile=/workspace/pipelight-misc/jacoco-report/jacoco.exec`
- **GradlePlugin**：
  - 命令追加 `jacocoTestReport`
  - 不强行改 Gradle 的输出路径（`build.gradle` 里 jacoco 插件可能有自定义配置，`-P` 注入不可靠）
  - 命令**末尾**追加 copy 把 Gradle 默认产物统一到 pipelight-misc：
    ```sh
    mkdir -p /workspace/pipelight-misc/jacoco-report && \
    cp build/jacoco/test.exec /workspace/pipelight-misc/jacoco-report/jacoco.exec 2>/dev/null || true && \
    cp build/reports/jacoco/test/jacocoTestReport.xml /workspace/pipelight-misc/jacoco-report/jacoco.xml 2>/dev/null || true
    ```
  - 如果项目 jacoco 插件输出路径被自定义（非默认），cp 静默失败，jacoco step 的 "no exec file" 分支会 skip 并提示用户调整配置

实现 `StepDef` trait 的所有方法（`config` / `exception_mapping` / `match_exception` / `output_report_str`），其他方法均转发给 `inner`。

### 4. MavenJacocoStep / GradleJacocoStep（增量）

结构和 `pmd_step` 对称：

```rust
pub struct MavenJacocoStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,        // 影响是否需要自己跑 jacococli report
}
```

**`config()` 产出命令（伪代码）**：

```sh
cd_prefix && \
# 1. 检查配置
if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then
  echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - ...'>&2; exit 1;
fi

# 2. 拿 git-diff 变更
CHANGED=$(cat git-diff-report/*.txt | sort -u | grep -E '\.(java|kt)$')
if [ -z "$CHANGED" ]; then echo 'jacoco: no changed java/kt files — skipping'; exit 0; fi

# 3. 应用 exclude patterns
FILTERED=$(echo "$CHANGED" | awk -f filter_by_excludes.awk)  # 读 yaml exclude 列表
if [ -z "$FILTERED" ]; then echo 'jacoco: all changed files excluded — skipping'; exit 0; fi

# 4. 检查 exec 文件
if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then
  echo 'jacoco: no jacoco.exec (test may have crashed) — skipping'; exit 0;
fi

# 5. 生成 jacoco.xml (仅 Standalone 和 MavenPlugin 需要；GradlePlugin 已在 test step 生成)
if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.xml ]; then
  # 下载/用 cache 的 jacococli.jar
  JACOCOCLI=$HOME/.pipelight/cache/jacoco-0.8.12/lib/jacococli.jar
  # ...download if missing...
  java -jar $JACOCOCLI report jacoco.exec \
       --classfiles target/classes --sourcefiles src/main/java \
       --xml pipelight-misc/jacoco-report/jacoco.xml
fi

# 6. awk 解析 XML，逐文件统计 LINE 覆盖率
#    XML 结构: <sourcefile name="X.java"><counter type="LINE" missed="M" covered="C"/>
#    写出三个报告:
#      jacoco-summary.txt  (全部文件 + 百分比)
#      uncovered.txt       (每个文件未覆盖行号范围，配合 <line nr="N" mi="X"/> 解析)
#      threshold-fail.txt  (<70% 的文件)

# 7. 判定
FAIL_COUNT=$(wc -l < threshold-fail.txt)
if [ "$FAIL_COUNT" -gt 0 ]; then
  echo "JaCoCo Total: $FAIL_COUNT files below 70%"
  exit 1
fi
exit 0
```

**`exception_mapping()`**：

```rust
ExceptionMapping::new(CallbackCommand::RuntimeError)
  .add("coverage_below_threshold", ExceptionEntry {
      command: CallbackCommand::AutoFix,
      max_retries: 9,
      context_paths: vec![
          "pipelight-misc/jacoco-report/jacoco.xml",
          "pipelight-misc/jacoco-report/uncovered.txt",
          "pipelight-misc/jacoco-report/jacoco-summary.txt",
          "pipelight-misc/jacoco-report/threshold-fail.txt",
          "pipelight-misc/jacoco-config.yml",
          "pipelight-misc/git-diff-report/staged.txt",
          "pipelight-misc/git-diff-report/unstaged.txt",
          "pipelight-misc/git-diff-report/untracked.txt",
          "pipelight-misc/git-diff-report/unpushed.txt",
      ],
  })
  .add("config_not_found", ExceptionEntry {
      command: CallbackCommand::AutoGenJacocoConfig,
      max_retries: 9,
      context_paths: self.source_paths.clone(),
  })
```

**`match_exception()`**：
- stderr 含 `PIPELIGHT_CALLBACK:auto_gen_jacoco_config` → `"config_not_found"`
- stdout 含 `JaCoCo Total:` → `"coverage_below_threshold"`

### 5. MavenJacocoFullStep / GradleJacocoFullStep（全仓）

差异点（其余复用增量 step 的模板）：
- 不过滤 git-diff，扫全仓 `*.java`/`*.kt`（但仍应用 exclude patterns）
- 报告目录改为 `pipelight-misc/jacoco-full-report/` 避免和增量互相覆盖
- `StepConfig`：`tag: "full"`，`active: false`，`allow_failure: true`
- exception_mapping 的 `coverage_below_threshold` 映射到 `JacocoPrintCommand`（不走 AutoFix）

### 6. Strategy 组装变化

**`maven/mod.rs`**：

```rust
// 在构建 test_info 时决定 JaCoCo 模式
let jacoco_mode = if info.quality_plugins.contains(&"jacoco".to_string()) {
    JacocoMode::MavenPlugin
} else {
    JacocoMode::Standalone {
        agent_jar_path: "~/.pipelight/cache/jacoco-0.8.12/lib/jacocoagent.jar".into(),
    }
};

// 包两层 decorator: 先 JacocoAgentTestStep，再 MavenCachedStep
let test_step_inner = TestStep::new(&test_info)
    .with_parser(parse_maven_test)
    .with_allow_failure(true)
    .with_test_report_globs(...)
    .with_failure_markers(...);
let test_step = JacocoAgentTestStep::new(Box::new(test_step_inner), jacoco_mode);
steps.push(Box::new(MavenCachedStep::wrap_with_deps(Box::new(test_step), vec![prev])));
prev = "test".into();

// 插入 jacoco 两个 step
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

// package 从依赖 test 改成依赖 jacoco_full
steps.push(Box::new(MavenCachedStep::wrap_with_deps(
    Box::new(package_step::PackageStep::new(info)),
    vec![prev],
)));
```

**`gradle/mod.rs`**：test 包 decorator 后在末尾追加两个 step，没有 package 步骤要调整。

## 数据流（端到端）

```
① 用户改 UserService.java，没补测试，运行 pipelight run

② DAG:
   build → ... → pmd_full → test(JacocoAgentTestStep) → jacoco → jacoco_full → package

③ test step 执行:
   - 标准模式: export MAVEN_OPTS=-javaagent:...=destfile=pipelight-misc/jacoco-report/jacoco.exec
   - 插件模式: 命令前置 jacoco:prepare-agent
   - 正常跑 mvn test / ./gradlew test
   - 测试结束: jacoco.exec 落盘

④ jacoco step 执行:
   a. 检查 jacoco-config.yml → 缺 → AutoGenJacocoConfig
   b. 拼 git-diff 变更 Java/Kotlin 文件
   c. 按 exclude patterns 过滤
   d. 若 filtered 为空 → skipping exit 0
   e. 若 jacoco.exec 缺失 → skipping exit 0 (FailAndSkip 语义)
   f. 生成 jacoco.xml (Standalone/MavenPlugin 需要；GradlePlugin 已生成)
   g. awk 解析 XML，逐文件算 LINE%
   h. 写 summary / uncovered / threshold-fail 三个报告
   i. 有不达标 → exit 1 + echo "JaCoCo Total: N files below 70%"

⑤ pipelight 匹配 coverage_below_threshold → AutoFix → 发回调 JSON 给 LLM

⑥ LLM 读 context_paths (XML/uncovered/summary/config/git-diff)
   + 读变更源码 + src/test/** 现有测试风格
   → 在 src/test/java/... 补 UT 方法
   → pipelight retry --step jacoco (实际会从 test step 开始整条链重跑)

⑦ 回到 ③ 循环，直到通过或 9 次耗尽

⑧ jacoco_full (默认 inactive；--full-report-only 激活):
   - 扫全仓，应用 exclude，生成 jacoco-full-report/*
   - 有 <70% → JacocoPrintCommand → LLM 打印按 package 分组的汇总表
   - allow_failure=true，不阻塞 package
```

## 边界情况处理

| 场景 | 行为 |
|---|---|
| 无 *.java/*.kt 变更 | jacoco self-skip (exit 0) |
| jacoco-config.yml 缺失 | `AutoGenJacocoConfig` → LLM 生成 |
| 变更文件全被 exclude 命中 | jacoco self-skip (exit 0) |
| test 完全崩溃，无 jacoco.exec | FailAndSkip (exit 0 + 原因) |
| test 有 failure 但产生了 exec | 照常判断覆盖率（test allow_failure=true 不会阻塞 pipeline） |
| jacoco.exec 存在但 XML 解析不出 sourcefile | FailAndSkip + 打印排查提示 |
| pmd/spotbugs 一样的 `--full-report-only` 激活语义 | jacoco 两 step 跟随同一开关 |
| jacococli.jar 下载失败（网络问题） | 报 runtime_error（和 PMD CLI 下载失败行为一致） |

## 配置文件：jacoco-config.yml

首次缺失时由 LLM 通过 `AutoGenJacocoConfig` 回调生成，默认内容：

```yaml
# JaCoCo 覆盖率检查配置
# 被 pipelight 的 jacoco / jacoco_full step 读取

# 每文件 LINE 覆盖率下限（百分比）
threshold: 70

# 排除在覆盖率检查外的文件 glob (相对项目根)
# LLM 应根据项目实际特征扩充这里
exclude:
  - "**/*Dto.java"
  - "**/*DTO.java"
  - "**/*Config.java"
  - "**/*Configuration.java"
  - "**/*Exception.java"
  - "**/*Application.java"
  - "**/generated/**"
```

## 测试策略

**单元测试**（每个新文件都有 `#[cfg(test)] mod tests`）：

- **Detector**：
  - `detect_jacoco_in_pom` / `detect_jacoco_absent_in_pom`
  - `detect_jacoco_in_build_gradle`（groovy + kts）
- **CallbackCommand**：
  - `serde_roundtrip` 覆盖新增两个 variant
  - `registry_built_in_commands` 验证 `AutoGenJacocoConfig→Retry`、`JacocoPrintCommand→JacocoPrint`
- **JacocoStep / JacocoFullStep**：
  - `jacoco_step_config_not_found_triggers_auto_gen`
  - `jacoco_step_coverage_below_triggers_auto_fix`
  - `jacoco_full_step_uses_print_command`
  - `jacoco_step_command_contains_callback`
  - `jacoco_step_reads_git_diff_report`
  - `jacoco_step_filters_config_excludes`
  - `jacoco_step_handles_missing_exec_file`
  - `jacoco_plugin_mode_uses_jacoco_prepare_agent`
  - `jacoco_standalone_mode_downloads_agent_jar`
- **JacocoAgentTestStep decorator**：
  - `standalone_mode_injects_javaagent_in_maven_opts`
  - `maven_plugin_mode_injects_jacoco_prepare_agent_goal`
  - `gradle_plugin_mode_appends_jacoco_test_report`
  - `jacoco_mode_none_leaves_test_command_unchanged`（回归保护）
- **Strategy 组装**（扩展现有 `test_maven_steps_with_checkstyle` 等）：
  - `maven_dag_has_jacoco_after_test_before_package`
  - `gradle_dag_has_jacoco_at_end`
  - `package_depends_on_jacoco_full`
  - `jacoco_full_tagged_as_full_and_inactive_by_default`

**手工端到端验证**（不纳入自动化）：
1. 拿一个 Java 8 的 Spring Boot 样例项目，跑 `pipelight run`，验证 jacoco 生效并能触发 AutoFix
2. 拿一个 Java 17 的 Gradle 项目同样验证
3. 拿一个已经配了 `jacoco-maven-plugin` 的项目，验证走插件模式

## 和 skill 的同步

按 `CLAUDE.md` 的规则，修改 `src/ci/callback/command.rs` 和 `action.rs` 新增枚举值后，**必须同步更新** `global-skills/pipelight-run/SKILL.md`：

1. 在「回调命令处理表」添加两行：
   - `AutoGenJacocoConfig` / action=Retry / LLM 操作：搜索项目特征，生成 `pipelight-misc/jacoco-config.yml`
   - `JacocoPrintCommand` / action=JacocoPrint / LLM 操作：解析 `pipelight-misc/jacoco-full-report/jacoco.xml`，打印按 package 分组的覆盖率汇总表
2. 新增 **`AutoGenJacocoConfig` 详细流程** 章节（和 `auto_gen_pmd_ruleset` 对称，说明 LLM 如何判断排除规则、默认阈值）
3. 新增 **`JacocoPrintCommand` 详细流程** 章节（打印格式模板：文件 / 覆盖率 / 未覆盖行数）
4. 执行 `cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/` 同步到本地

## 风险与后续

- **Shell awk 解析脆弱性**：JaCoCo XML 格式稳定（跟版本锁定 0.8.12），但如果未来升级版本需要回归测试。
- **test step 重跑成本**：每次 AutoFix 循环都重跑整个 test 套件。若项目测试很慢（>5min），9 次循环 ≈ 45+min，用户可能会手动 kill。考虑后续加 `--jacoco-retries N` CLI 选项覆盖默认值（本期不做）。
- **JaCoCo 插件版本冲突**：项目自带的 jacoco-maven-plugin 版本可能和我们期望的 0.8.12 不同。插件模式下我们不强行升级插件版本，认命用户项目自带版本；只有 Standalone 模式严格锁 0.8.12。
- **Gradle 子模块 / multi-project**：当前设计默认单模块。多模块项目的 exec 聚合是另一个层次的问题，本期暂不处理（落地时跑通后再看）。
