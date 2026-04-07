# Pipelight

轻量级 CLI CI/CD 工具，通过 Docker 容器隔离执行流水线步骤。配合 Claude Code Skills 实现 AI 驱动的自动化开发工作流。

## 快速开始（Claude Code 用户）

只需两步：

### Step 1: 克隆并同步环境

```bash
git clone git@github.com:amwtke/deeparch_cicd.git
cd deeparch_cicd
```

在 Claude Code 中运行：

```
/pipelight-sync
```

自动完成：环境检查 → Rust/Docker 安装 → 编译 pipelight → 运行测试 → 安装到 PATH → 同步 Skills → 加载知识库。完成后即可在任意项目中使用 `pipelight` 命令。

### Step 2: 在目标项目中运行 CI

切换到需要 CI 的项目目录，在 Claude Code 中运行：

```
/pipelight-run
```

自动完成：检测项目类型 → 生成 pipeline.yml → Docker 容器内执行 build/test/lint → 失败时 AI 自动修复并重试。

#### /pipelight-run 参数

| 参数 | 说明 | 示例 |
|------|------|------|
| `--reinit` | 重新检测项目并覆盖 pipeline.yml | `/pipelight-run --reinit` |
| `--skip <steps>` | 跳过指定 step（逗号分隔） | `/pipelight-run --skip spotbugs,pmd` |
| `--step <name>` | 只运行指定 step | `/pipelight-run --step build` |
| `--dry-run` | 只显示执行计划，不实际运行 | `/pipelight-run --dry-run` |
| `--verbose` | 显示容器内全量输出 | `/pipelight-run --verbose` |

参数可组合使用：`/pipelight-run --reinit --skip pmd --verbose`

#### /pipelight-run 失败处理流程

```
pipeline 执行 → 成功？ → 报告结果
                  ↓ 失败
          策略是 auto_fix？
           ↓ 是          ↓ 否
   读取错误日志      报告失败，终止
   AI 分析修复代码
   retries > 0？
    ↓ 是     ↓ 否
  pipelight retry  报告失败
```

## 核心理念：Debug-First CI

传统 CI/CD 工具在构建失败时只做一件事——**把错误日志甩给你**，然后等你手动修复、重新提交、再次触发流水线。这个循环完全依赖人：

```
传统 CI:  代码提交 → CI 构建 → 失败 → 人读日志 → 人改代码 → 人重新提交 → CI 再跑 → ...
```

Pipelight 的做法不同。当你在 Claude Code 中运行 `/pipelight-run`，失败不是终点，而是 **LLM 自动修复的起点**：

```
Pipelight:  /pipelight-run → CI 构建 → 失败 → LLM 读日志 → LLM 改代码 → pipelight retry → 通过
```

这就是 **Debug-First**：CI 流水线本身内置了"出错就修"的能力，而不是把调试的负担推回给开发者。`pipeline.yml` 中的 `on_failure: auto_fix` 配置告诉 pipelight "这个 step 失败时，把错误上下文交给 AI，让它尝试修复"。修复后自动 retry，直到通过或耗尽重试次数。

对于开发者来说，体验从"盯着红色日志找 bug"变成了"喝杯咖啡等结果"。

### 手动使用（不依赖 Claude Code）

```bash
# 安装
cargo build --release
cp target/release/pipelight /usr/local/bin/

# 在目标项目中
pipelight init                  # 生成 pipeline.yml
pipelight run                   # 运行流水线
pipelight run --verbose         # 查看容器内全量输出
pipelight validate              # 验证 pipeline.yml 语法
pipelight list                  # 列出所有 step
pipelight run --dry-run         # 只看执行计划
pipelight status --run-id <id>  # 查看某次运行的状态
pipelight retry --run-id <id> --step <name>  # 重试失败的 step
```

### 输出模式

| 模式 | 用法 | 场景 |
|------|------|------|
| TTY (默认) | 自动检测终端 | 人在终端交互，有进度条和颜色 |
| Plain | `--output plain` 或管道重定向时自动 | CI 环境、输出到文件 |
| JSON | `--output json` | Claude Code headless、程序消费 |

```bash
pipelight run --output json    # JSON 输出，含 test_summary 字段
pipelight run --output plain   # 纯文本，无颜色
pipelight run > build.log      # 自动切换到 plain 模式
```

## 支持的项目类型

| 类型 | 检测文件 | Docker 镜像 | 特有 Step |
|------|---------|-------------|----------|
| Maven | `pom.xml` | `maven:3.9-eclipse-temurin-{8,11,17,21}` | checkstyle, package |
| Gradle | `build.gradle` / `build.gradle.kts` | `gradle:8-jdk{8,11,17,21}` | checkstyle |
| Rust | `Cargo.toml` | `rust:latest` | clippy |
| Node.js | `package.json` | `node:{version}-slim` | typecheck (TypeScript) |
| Python | `pyproject.toml` / `requirements.txt` | `python:{version}-slim` | mypy |
| Go | `go.mod` | `golang:{version}` | vet |

## pipeline.yml 示例

```yaml
name: maven-java-ci
steps:
  - name: build
    image: maven:3.9-eclipse-temurin-17
    commands:
      - mvn compile -q
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/main/java/
        - pom.xml

  - name: test
    image: maven:3.9-eclipse-temurin-17
    commands:
      - mvn test
    depends_on:
      - build
    on_failure:
      strategy: notify

  - name: package
    image: maven:3.9-eclipse-temurin-17
    commands:
      - mvn package -DskipTests
    depends_on:
      - test
```

## 运行效果

```
[build] Starting... (maven:3.9-eclipse-temurin-17)
[build] OK (47.4s)
[test] Starting... (maven:3.9-eclipse-temurin-17)
[test] OK (83.2s)
[package] Starting... (maven:3.9-eclipse-temurin-17)
[package] OK (30.1s)

Test Summary: 42 passed, 0 failed, 2 skipped

Step             Duration     Status
build            47.4s        OK
test             83.2s        OK
package          30.1s        OK
Total            160.7s
```

## 失败处理策略

| 策略 | 行为 |
|------|------|
| `auto_fix` | 退出码 1，保存状态，等待 LLM agent 修复后 `pipelight retry` |
| `abort` | 退出码 2，终止流水线 |
| `notify` | 退出码 2，通知用户 |

## 开发

```bash
cargo build          # 编译
cargo test           # 运行测试
cargo run -- --help  # 查看帮助
```
