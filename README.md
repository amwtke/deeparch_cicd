# Pipelight

轻量级 CLI CI/CD 工具，通过 Docker 容器隔离执行流水线步骤。

## 快速开始

### 前置条件

- Rust (rustup)
- Docker (daemon 必须在运行)

### 安装

```bash
git clone git@github.com:amwtke/deeparch_cicd.git
cd deeparch_cicd
cargo build --release
```

编译后的二进制在 `target/release/pipelight`，可以复制到 PATH 中：

```bash
cp target/release/pipelight /usr/local/bin/
```

### 使用

#### 1. 生成 pipeline.yml

在你的项目目录下运行：

```bash
pipelight init
```

自动检测项目类型（Maven/Gradle/Rust/Node/Python/Go），生成 `pipeline.yml`。

也可以指定目录：

```bash
pipelight init --dir /path/to/your/project
```

#### 2. 运行流水线

```bash
pipelight run
```

带实时日志和进度条。加 `--verbose` 查看容器内全量输出：

```bash
pipelight run --verbose
```

#### 3. 其他命令

```bash
pipelight validate          # 验证 pipeline.yml 语法
pipelight list              # 列出所有 step
pipelight run --dry-run     # 只看执行计划，不实际运行
pipelight status --run-id <id>   # 查看某次运行的状态
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

## 开发环境同步 (pipelight-sync)

在 Claude Code 中使用 `/pipelight-sync` 命令，一键完成新机器的开发环境准备或已有机器的状态同步。

### 功能

| 步骤 | 说明 |
|------|------|
| Git 同步 | 自动提交本地未保存的改动，`git pull --rebase`，推送未推送的 commit |
| 环境检查 | 检查 Rust、Cargo、Docker、Git、Claude Code 是否安装且可用；缺失时自动安装 Rust |
| 增量构建 | 对比上次构建的 commit，仅在 `src/`、`Cargo.toml`、`Cargo.lock` 有变更时才 `cargo build --release` |
| 单元测试 | 构建成功后自动运行 `cargo test`，失败则中止 |
| 安装二进制 | 将 `pipelight` 复制到 `/usr/local/bin` 或 `~/.cargo/bin`，全局可用 |
| 全局 Skills 安装 | 将 `global-skills/` 下的 skill 同步到 `~/.claude/skills/`，所有项目均可使用 |
| 知识库加载 | 读取 `docs/` 下的 vision、architecture、decisions、dev-environment 文档，恢复项目上下文 |

### 用法

在 Claude Code 对话中直接输入：

```
/pipelight-sync
```

无需任何参数。命令执行完毕后会输出汇总表：

```
=== Pipelight Sync Complete ===

Git:
  repo         OK (up to date)

Environment:
  rustc        OK 1.94.1
  cargo        OK 1.94.1
  docker       OK 29.1.5 (daemon running)
  git          OK 2.43.0
  claude       OK 2.1.90

Build:
  cargo build  OK (release)
  cargo test   OK (154 passed)
  pipelight    OK 0.1.0 (installed)

Global Skills:
  pipelight-run  OK

Knowledge:
  docs/        OK (5 documents loaded)

Ready to develop!
```

### 适用场景

- 切换到新开发机器时
- 长时间未开发后恢复上下文
- 拉取队友代码后确保本地环境一致

## 开发

```bash
cargo build          # 编译
cargo test           # 运行测试
cargo run -- --help  # 查看帮助
```
