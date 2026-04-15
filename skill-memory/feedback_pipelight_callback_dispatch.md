---
name: pipelight 必须逐 step 遍历 on_failure
description: 处理 pipelight JSON 输出时，必须先枚举所有 step 的非 null on_failure 并按 action 完整执行（不是只提一嘴），再看 pipeline 整体 status
type: feedback
originSessionId: 348d494e-b5de-42ac-8883-3df123ce9dee
---
收到 pipelight JSON 后，第一步永远是：列出所有 step 的非 null `on_failure`，逐条按 skill 表 dispatch 对应 **action**（包括 `auto_gen_pmd_ruleset` / `test_print` / `spotbugs_print` / `pmd_print` 等），不论 step 的 status 是 success/failed/retryable，也不论 pipeline 整体 status，也不论是否在 `--full-report-only` 模式。**dispatch 必须是"完整执行 action 详细流程"，不是"提一嘴该 action 触发了"**：
- `test_print` → 按 `context_paths` 里的 glob 解析 JUnit XML，**按模块聚合**打印 Tests/Passed/Failed/Errors/Skipped 的 Markdown 表格，失败模块用 ✗ 前缀
- `pmd_print` / `spotbugs_print` → 读 report 文件打印分类统计表
- `auto_fix` / `auto_gen_pmd_ruleset` → 按对应详细流程改代码或生成配置，然后 retry
- `git_fail` / `fail_and_skip` → 无操作，pipelight 已自动 skip

**Why:**
1. 2026-04-14 跑 wyproject-master --full-report-only，pmd_full 返回 `auto_gen_pmd_ruleset`，我因 pipeline 整体 success 就跳过 ruleset 生成。
2. 2026-04-15 跑 rc 项目，test step 触发 `test_print` action，我只贴了 Gradle 的 6 条失败 task 摘要没解析 JUnit XML，用户两次追问（"share-support 有跑吗" / "test_print 是按模块打印吗"）才补出按模块聚合的表格——**半执行等于没执行**。CLAUDE.md 里把这类"打印型 action 在 status=success 时被遗漏"明确列为"禁止再犯"的历史错误，我又重复了。

**How to apply:** 解析 JSON 第一步：
1. `for s in steps: print(s.name, s.on_failure.action)` 列清单
2. 对每个非 null action，**在响应中产出该 action 规定的具体产物**（表格 / 文件修改 / retry 命令），而不是只说"该 action 已触发"
3. 再走 status 分支决定是否 retry

每个 action 要写到哪个文件 / 打印什么格式，去 `~/.claude/skills/pipelight-run/SKILL.md` 的"回调命令处理表"和对应"详细流程"章节查，别凭记忆。
