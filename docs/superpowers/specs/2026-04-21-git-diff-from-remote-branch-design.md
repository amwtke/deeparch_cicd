# git-diff-from-remote-branch Flag 设计稿

- **日期**：2026-04-21
- **作者**：xiaojin（brainstorming with Claude）
- **状态**：Draft → Awaiting Review

## 背景与目标

现有的 `git-diff` step 用来收集"本地 + 未推送"的变更文件清单，供 PMD / SpotBugs / JaCoCo 做增量扫描。它硬编码用 `@{upstream}` 作为"branch-ahead" 的对比基准。

**问题**：典型的 feature/bugfix 分支工作流是"从 `origin/main` 切出分支 → 开发 → 推一些 commit 到 `origin/feat/x` → 继续改"。此时 `@{upstream}` = `origin/feat/x`，增量扫描只覆盖**未推送的**那几个 commit，**已经推到 feat 分支的 commit 不在扫描范围内**。用户希望能以"从迁出至今所有改动"为粒度跑代码质量检查。

**目标**：

1. 新增 CLI flag `--git-diff-from-remote-branch=<remote-branch>`，允许显式指定一个 remote ref（如 `origin/main`）作为 branch-ahead 对比基准，替代 `@{upstream}`。
2. 借机把 git-diff step 的输出从 4 个 txt 文件重构为单个 `diff.txt`（去重的路径集合），精简下游消费逻辑，提升可扩展性。
3. 不改变默认行为（不传 flag 时等价于今天的 `@{upstream}` 模式），保证向后兼容。

## 关键决策（已与用户确认）

| # | 决策点 | 选择 | 备选（已否决） |
|---|---|---|---|
| 1 | `diff.txt` 格式 | **X — 纯路径去重列表**，分类统计只在 stdout 输出 | Y（带分类前缀，日志型）、Z（两段式 header+path） |
| 2 | 默认行为 | **A — 保守**：不传 flag 保持现有 `@{upstream}` | B（自动检测 main/master/develop）、C（flag 支持 `auto`） |
| 3 | flag 指定时的语义 | **REPLACE** — branch-ahead 只算 `BASE..HEAD`，不再看 `@{upstream}` | ADD（两者并存，冗余） |
| 4 | 错误处理：ref 不存在 | **硬失败**，退出码 2，映射到 `RuntimeError` 回调 | 自动 fetch、静默 fallback |
| 5 | flag 值格式 | 要求完整 ref `origin/<branch>` | 裸名 `main`（多 remote 歧义） |
| 6 | 参数流 | **in-memory pipeline 改写**（跟 `full_report_only` 同构） | 脚本读 env var、重新生成 pipeline.yml |
| 7 | RunState 持久化 | `git_diff_base: Option<String>` 持久化，retry 时回灌 | 仅内存态 |

## 架构总览

### 参数流

```
CLI (clap)
  └─ --git-diff-from-remote-branch=origin/main
       ↓
cmd_run(...)
  ├─ state.git_diff_base = Some("origin/main")  [持久化]
  └─ 遍历 pipeline.steps，找到 name == "git-diff"，
     用 GitDiffStep::with_base_ref(Some("origin/main")).config().commands[0]
     覆写该 step 的 commands[0]
       ↓
Scheduler → Executor (local step)
  └─ 执行新脚本 → 生成 pipelight-misc/git-diff-report/diff.txt
       ↓
下游 PMD / SpotBugs / JaCoCo step
  └─ 通过 git_changed_files_snippet() 读 diff.txt → 得到 CHANGED_FILES
```

### git-diff step 输出变化

| 文件 | 现状 | 新设计 |
|---|---|---|
| `pipelight-misc/git-diff-report/unstaged.txt` | ✅ 4 个独立文件 | ❌ 移除 |
| `pipelight-misc/git-diff-report/staged.txt` | ✅ | ❌ 移除 |
| `pipelight-misc/git-diff-report/untracked.txt` | ✅ | ❌ 移除 |
| `pipelight-misc/git-diff-report/unpushed.txt` | ✅ | ❌ 移除 |
| `pipelight-misc/git-diff-report/diff.txt` | — | ✅ **新增**：所有分类的路径 union + sort -u |

分类统计仍在 step stdout 里打印，供 GitDiffReport callback 给 LLM 做人类可读报告用。

## 详细设计

### 1. CLI 层（`src/cli/mod.rs`）

#### Run 子命令新增 flag

```rust
/// Use the given remote ref (e.g. `origin/main`) as the base for computing
/// branch-ahead changes, instead of the configured `@{upstream}`. Useful for
/// running code-quality checks on ALL files changed since the branch was
/// cut from a mainline branch.
#[arg(long = "git-diff-from-remote-branch", value_name = "REMOTE_BRANCH")]
git_diff_from_remote_branch: Option<String>,
```

**flag 在 Run 枚举中的位置**：紧跟在 `full_report_only` 之后（两者语义相邻——一个全扫，一个换 base）。

#### cmd_run 签名扩展

```rust
async fn cmd_run(
    file: PathBuf,
    step_filter: Option<String>,
    skip_steps: Vec<String>,
    dry_run: bool,
    output: Option<String>,
    run_id: Option<String>,
    verbose: bool,
    ping_pong: bool,
    full_report_only: bool,
    git_diff_base: Option<String>,   // ← 新增
) -> Result<i32> {
    ...
    // 在 pipeline 加载后、scheduler 构造前：
    if let Some(ref base) = git_diff_base {
        if let Some(step) = pipeline.steps.iter_mut().find(|s| s.name == "git-diff") {
            let new_cfg = GitDiffStep::with_base_ref(Some(base.clone())).config();
            step.commands = new_cfg.commands;
        }
    }

    state.git_diff_base = git_diff_base.clone();
    ...
}
```

#### Retry 子命令对称扩展

`Retry` 子命令也加同名 flag；`cmd_retry` 里：
- 若用户传了 flag → 用新值覆盖 run_state 里原值
- 若未传 → 从 run_state 读回上次的 `git_diff_base`
- 无论哪种，都对 in-memory pipeline 再做一次 commands 覆写

### 2. RunState 层（`src/run_state/mod.rs`）

```rust
pub struct RunState {
    ...
    pub full_report_only: bool,
    pub git_diff_base: Option<String>,   // ← 新增
    ...
}
```

serde 默认值：`None`（老的 run_state 文件解析成 None，向后兼容）。

### 3. GitDiffStep 重构（`src/ci/pipeline_builder/base/git_diff_step.rs`）

#### 结构体与构造

```rust
pub struct GitDiffStep {
    /// None → 使用 @{upstream}（向后兼容默认）
    /// Some → 使用指定 ref（如 "origin/main"）作为 branch-ahead base
    base_ref: Option<String>,
}

impl GitDiffStep {
    pub fn new() -> Self { Self { base_ref: None } }
    pub fn with_base_ref(base_ref: Option<String>) -> Self { Self { base_ref } }
}
```

`pipeline_builder::mod.rs` 中 `step_defs_for_pipeline` 和 `generate_pipeline` 里仍调用 `GitDiffStep::new()`——生成时不知道 CLI flag。CLI 侧改写 commands 即可。

#### config() 生成的脚本

两个变体**只在开头 BASE 赋值差异**：

**变体 A（默认，`base_ref == None`）**：
```bash
BASE=$(git rev-parse --abbrev-ref --symbolic-full-name @{upstream} 2>/dev/null || true)
BASE_LABEL="@{upstream}"
```

**变体 B（`base_ref == Some("origin/main")`）**：
```bash
BASE="origin/main"
BASE_LABEL="origin/main"
```

剩余的脚本体完全一致：

```bash
# pipelight:git-diff-step v2
if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo 'git-diff: not a git repository — skipping'; exit 0
fi
REPORT_DIR=pipelight-misc/git-diff-report
mkdir -p "$REPORT_DIR"

# ... BASE / BASE_LABEL 赋值（两变体各自的那两行）...

UNSTAGED=$(git diff --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
STAGED=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)
UNTRACKED=$(git ls-files --others --exclude-standard 2>/dev/null | while read f; do [ -f "$f" ] && echo "$f"; done)

BRANCH_AHEAD=""
BRANCH_AHEAD_ERR=0
if [ -n "$BASE" ]; then
  if ! git rev-parse --verify "$BASE" >/dev/null 2>&1; then
    echo "git-diff: base ref '$BASE' not found — run 'git fetch' first" >&2
    BRANCH_AHEAD_ERR=1
  else
    BRANCH_AHEAD=$(git diff "$BASE"..HEAD --name-only --diff-filter=ACMR 2>/dev/null \
                   | while read f; do [ -f "$f" ] && echo "$f"; done)
  fi
fi

U=$(printf '%s\n' "$UNSTAGED"     | sed '/^$/d' | wc -l | tr -d ' ')
S=$(printf '%s\n' "$STAGED"       | sed '/^$/d' | wc -l | tr -d ' ')
T=$(printf '%s\n' "$UNTRACKED"    | sed '/^$/d' | wc -l | tr -d ' ')
B=$(printf '%s\n' "$BRANCH_AHEAD" | sed '/^$/d' | wc -l | tr -d ' ')

{
  printf '%s\n' "$UNSTAGED"
  printf '%s\n' "$STAGED"
  printf '%s\n' "$UNTRACKED"
  printf '%s\n' "$BRANCH_AHEAD"
} | sed '/^$/d' | sort -u \
  | while read f; do [ -f "$f" ] && echo "$f"; done \
  > "$REPORT_DIR/diff.txt"

TOTAL=$(wc -l < "$REPORT_DIR/diff.txt" | tr -d ' ')

if [ "$BRANCH_AHEAD_ERR" = "1" ]; then exit 2; fi

if [ "$TOTAL" -eq 0 ]; then
  echo 'git-diff: working tree clean and no branch-ahead commits — skipping'; exit 0
fi

echo "git-diff: $TOTAL unique file(s) changed on current branch"
echo "  unstaged: $U"
echo "  staged: $S"
echo "  untracked: $T"
if [ -n "$BASE" ]; then
  echo "  branch-ahead (vs $BASE_LABEL): $B"
else
  echo "  branch-ahead: n/a (no base ref configured)"
fi
exit 1
```

sentinel 注释行 `# pipelight:git-diff-step v2` 放在脚本首行，为将来检测"用户是否手改过脚本"留钩子（本次不实现识别逻辑，只是埋点）。

#### 退出码约定

| exit | 语义 | 触发 match_exception |
|---|---|---|
| 0 | 无改动 / 非 git 仓库，静默跳过 | — |
| 1 | 有改动，正常报告 | `git_diff_changes_found` → `GitDiffCommand` |
| 2 | 显式 base ref 不存在 | `git_diff_base_not_found` → `RuntimeError` |

#### exception_mapping() 扩展

```rust
fn exception_mapping(&self) -> ExceptionMapping {
    ExceptionMapping::new(CallbackCommand::GitDiffCommand)
        .add(
            "git_diff_changes_found",
            ExceptionEntry {
                command: CallbackCommand::GitDiffCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/git-diff-report/diff.txt".into(),  // ← 单文件
                ],
            },
        )
        .add(
            "git_diff_base_not_found",
            ExceptionEntry {
                command: CallbackCommand::RuntimeError,   // 复用现有 action
                max_retries: 0,
                context_paths: vec![],
            },
        )
}

fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
    if exit_code == 2 && stderr.contains("base ref") && stderr.contains("not found") {
        return Some("git_diff_base_not_found".into());
    }
    if stdout.contains("unique file(s) changed on current branch") {
        return Some("git_diff_changes_found".into());
    }
    None
}
```

#### output_report_str() 更新

关键字 `"change record(s) on current branch"` → `"unique file(s) changed on current branch"`。

### 4. 下游消费者（`src/ci/pipeline_builder/mod.rs`）

`git_changed_files_snippet()` 简化：

```rust
pub fn git_changed_files_snippet(globs: &[&str], subdir: Option<&str>) -> String {
    let extensions: Vec<&str> = globs.iter().filter_map(|g| g.strip_prefix("*.")).collect();
    let grep_filter = if extensions.is_empty() {
        String::new()
    } else if extensions.len() == 1 {
        format!(" | grep -E '\\.{}$'", extensions[0])
    } else {
        format!(" | grep -E '\\.({})$'", extensions.join("|"))
    };

    let sed_strip = match subdir {
        Some(sd) => format!(" | sed 's|^{}/||'", sd),
        None => String::new(),
    };

    format!(
        "CHANGED_FILES=$( \
           cat /workspace/pipelight-misc/git-diff-report/diff.txt 2>/dev/null \
           {sed}{grep} \
           | while read f; do [ -f \"$f\" ] && echo \"$f\"; done \
         )",
        sed = sed_strip,
        grep = grep_filter,
    )
}
```

改动点：
- `cat` 只读一个 `diff.txt`（不再 cat 4 个文件）
- 去掉 `| sort -u`（diff.txt 已 sort -u）
- 其他保持不变（ext 过滤、subdir 剥离、文件存在性检查）

**影响范围**（间接）：所有调用 `git_changed_files_snippet` 的 step，无需改代码：

- `src/ci/pipeline_builder/maven/pmd_step.rs`
- `src/ci/pipeline_builder/maven/spotbugs_step.rs`
- `src/ci/pipeline_builder/maven/jacoco_step.rs`
- `src/ci/pipeline_builder/gradle/pmd_step.rs`
- `src/ci/pipeline_builder/gradle/spotbugs_step.rs`
- `src/ci/pipeline_builder/gradle/jacoco_step.rs`

## 边界与错误处理

### 场景 1：未配 `@{upstream}`，未传 flag（当前行为保留）
- `BASE` 为空 → `BRANCH_AHEAD=""` → stdout 输出 `branch-ahead: n/a (no base ref configured)`
- 其他三类（unstaged/staged/untracked）照常收集
- 退出 0（空）或 1（有改动）

### 场景 2：传了 flag，但 ref 本地不存在
- `git rev-parse --verify "$BASE"` 失败
- stderr: `git-diff: base ref 'origin/main' not found — run 'git fetch' first`
- 退出码 2 → match 到 `git_diff_base_not_found` → `RuntimeError` 回调 → pipeline 终止
- **不做** auto-fetch（网络 I/O 副作用不透明）
- **不做** fallback 到 `@{upstream}`（掩盖用户意图错误）

### 场景 3：flag 值合法但语义奇怪（`HEAD~3`、裸 `main`）
- 裸 `main` → `git rev-parse --verify main` 成功（本地分支）或失败（不存在）→ 按 ref 存在性处理
- `HEAD~3` → 技术上可用，当作高级用法不阻拦
- 文档建议使用 `origin/<branch>` 形式

### 场景 4：retry
- 首次 run 时 `state.git_diff_base` 写入 run_state
- `pipelight retry --run-id X` 从 run_state 读回 `git_diff_base`，对 in-memory git-diff step 再次改写 commands
- retry 命令自身也支持同名 flag（显式覆盖）

### 场景 5：用户手改过 pipeline.yml 的 git-diff 脚本
- CLI 改写会覆盖用户修改
- **本次不处理**；脚本首行的 sentinel 注释 `# pipelight:git-diff-step v2` 为后续检测留钩子
- 文档里注明"手改 pipeline.yml 中 git-diff step 的脚本不被支持"

## Skill 文档更新（`global-skills/pipelight-run/SKILL.md`）

### Flags 列表插入新条目

位置：在 `--full-report-only` 之后。

```markdown
- `--git-diff-from-remote-branch=<remote-branch>`: 指定远程分支作为 branch-ahead
  对比基准（如 `origin/main`）。用于在"从主分支切出的 feature 分支"上只对自迁出
  以来改动过的文件运行代码质量扫描。不传此 flag 时，pipelight 使用 `@{upstream}`
  作为 base（与原行为一致，仅覆盖本地未推送的 commit）。
  - 示例：`pipelight run --git-diff-from-remote-branch=origin/main`
  - ref 不存在时 pipeline 终止并提示 `git fetch`
```

### 回调命令处理表

本次复用现有 `RuntimeError` action，**表结构不变**。如果后续决定引入独立 `CallbackCommand::GitDiffBaseMissing`，再同步加行。

### 产物目录清单

若 skill 里列出了 `pipelight-misc/git-diff-report/` 下文件清单，改为只列 `diff.txt`。

### 同步命令

改完 `global-skills/pipelight-run/SKILL.md` 后执行：
```bash
cp -r global-skills/pipelight-run/ ~/.claude/skills/pipelight-run/
```

## 测试计划

### `git_diff_step.rs` 单元测试（调整既有 + 新增）

| 测试 | 操作 |
|---|---|
| `test_config_basic` | 保留 |
| `test_exception_mapping_default_is_git_diff_command` | 断言 `context_paths.len()` 4→1 |
| `test_exception_mapping_changes_found_key` | 文本改 `"unique file(s) changed"` |
| `test_registry_action_is_git_diff_report` | 保留 |
| `test_report_not_a_repo` | 保留 |
| `test_report_clean` | 文本改 `"branch-ahead commits"` |
| `test_report_has_changes` | 模拟输入文本同步更新 |
| `test_script_detects_untracked_files` | 保留 |
| `test_context_paths_include_untracked` | **删除**（语义消失） |
| `test_with_base_ref_none_uses_upstream` | **新增**：脚本含 `@{upstream}` 和 `BASE_LABEL="@{upstream}"` |
| `test_with_base_ref_some_uses_literal` | **新增**：脚本含 `BASE="origin/main"`，不含 `@{upstream}` |
| `test_script_writes_single_diff_txt` | **新增**：脚本写 `diff.txt`，无 4 个旧文件 |
| `test_exception_mapping_base_not_found_maps_to_runtime_error` | **新增**：新 key 映射到 `RuntimeError` action |
| `test_match_exception_base_not_found` | **新增**：exit=2 + stderr `"base ref ... not found"` → `git_diff_base_not_found` |

### `git_changed_files_snippet` 测试

| 测试 | 操作 |
|---|---|
| `test_snippet_reads_single_diff_txt` | **新增**：生成的片段只 cat `diff.txt`，不引用旧文件名 |

### CLI 集成测试（若仓库有对应 harness）

- 传 `--git-diff-from-remote-branch=origin/main` 时 pipeline 内存中 git-diff step 的 `commands[0]` 被改写为 literal 变体
- 不传 flag 时脚本保持 upstream 变体
- `RunState::git_diff_base` 在首次 run 后持久化，retry 时读回并重新应用

### 手工端到端验证

- 在 Java 仓库上实际跑 `pipelight run --git-diff-from-remote-branch=origin/main`
- 检查 `pipelight-misc/git-diff-report/diff.txt` 包含自 main 迁出以来所有改过的文件
- 确认 PMD / SpotBugs / JaCoCo 扫描范围相应扩大

## 迁移说明

- 这是 pipelight 生成的 pipeline.yml 的**破坏性变更**：新版 pipelight 生成的 git-diff step 脚本写 `diff.txt`，而下游 PMD/SpotBugs/JaCoCo 的脚本里 `cat` 的也是 `diff.txt`。
- 用户升级 pipelight 后需要 `pipelight clean && pipelight init` 重新生成 pipeline.yml（或手工同步）。
- 由于 pipeline.yml 一般包含 `git_credentials` 建议加入 `.gitignore`（CLAUDE.md 原则），实际多数用户不会版本化 pipeline.yml，重新生成代价低。
- 设计文档 / CHANGELOG 需要明确列出这一破坏性变更。

## 任务清单（初步）

1. `src/run_state/mod.rs` 新增 `git_diff_base: Option<String>` 字段
2. `src/cli/mod.rs` 新增 Run / Retry 的 flag，扩展 `cmd_run` / `cmd_retry` 签名
3. `src/ci/pipeline_builder/base/git_diff_step.rs` 重构：
   - 结构体与 `with_base_ref` 构造
   - 单一 `diff.txt` 输出脚本（两变体共享主体）
   - 新 exception key `git_diff_base_not_found`
   - `match_exception` / `output_report_str` 文本更新
4. `src/ci/pipeline_builder/mod.rs`：`git_changed_files_snippet` 简化为单文件读取
5. 更新所有 git-diff step 单测 + 新增覆盖 base_ref 变体的测试
6. 更新 `global-skills/pipelight-run/SKILL.md` flag 列表 + 产物目录清单，同步到 `~/.claude/skills/`
7. 手工端到端验证
8. CHANGELOG / README 说明迁移

## 开放问题 / 后续可做

- **自动检测 parent branch**（用户在 brainstorming 中提过，本稿否决）：若将来想做，可在 flag 值上支持 `auto` 关键字，按 `origin/main → master → develop` 启发式探测。
- **Sentinel 版本检测**：利用脚本首行 `# pipelight:git-diff-step v2` 在改写前检查是否匹配，避免覆盖用户手工修改。
- **多 base 并存场景**：如果用户同时关心"vs main" 和 "vs develop" 的差异，可考虑 flag 接受逗号分隔多值；本稿暂不支持。
