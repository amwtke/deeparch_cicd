# Pipelight

轻量级 CLI CI/CD 工具，用 Rust 实现，通过 Docker 容器隔离执行流水线步骤。配合 Claude Code Skills 实现 AI 驱动的自动化开发工作流。

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
| `--clean` | 清除所有 pipelight 产物（pipeline.yml + pipelight-misc/）后停止；与其他参数组合时先清除再继续 | `/pipelight-run --clean` |
| `--reinit` | 重新检测项目并覆盖 pipeline.yml | `/pipelight-run --reinit` |
| `--skip <steps>` | 跳过指定 step（逗号分隔） | `/pipelight-run --skip spotbugs,pmd` |
| `--step <name>` | 只运行指定 step | `/pipelight-run --step build` |
| `--dry-run` | 只显示执行计划，不实际运行 | `/pipelight-run --dry-run` |
| `--verbose` | 显示容器内全量输出 | `/pipelight-run --verbose` |
| `--list-steps` | 列出检测到的所有 step，不运行 | `/pipelight-run --list-steps` |

参数可组合使用：`/pipelight-run --clean --reinit --skip pmd --verbose`

#### /pipelight-run 失败处理流程

```
pipeline 执行 → 成功？ → 报告结果
                  ↓ 失败
          策略是 auto_fix？
           ↓ 是          ↓ 否
   读取错误日志      报告失败，终止
   (pipelight-misc/<step>.log)
   AI 分析修复代码
   retries > 0？
    ↓ 是     ↓ 否
  pipelight retry  报告失败
```

失败的 step 日志自动保存到 `pipelight-misc/<step>.log`，供 AI 或人工分析。

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
pipelight init                  # 自动检测项目类型，生成 pipeline.yml
pipelight run                   # 运行流水线
pipelight run --verbose         # 查看容器内全量输出
pipelight run --skip pmd,spotbugs  # 跳过指定 step
pipelight run --step build      # 只运行指定 step
pipelight run --dry-run         # 只看执行计划
pipelight validate              # 验证 pipeline.yml 语法
pipelight list                  # 列出 pipeline.yml 中所有 step
pipelight --list-steps          # 自动检测项目并列出所有 step（无需 pipeline.yml）
pipelight --list-steps --dir ./subproject  # 指定子目录
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
| Maven | `pom.xml` | `maven:3.9-eclipse-temurin-{8,11,17,21}` | checkstyle, spotbugs, pmd, package |
| Gradle | `build.gradle` / `build.gradle.kts` | `gradle:8-jdk{8,11,17,21}` | checkstyle, spotbugs, pmd |
| Rust | `Cargo.toml` | `rust:latest` | clippy |
| Node.js | `package.json` | `node:{version}-slim` | typecheck (TypeScript) |
| Python | `pyproject.toml` / `requirements.txt` | `python:{version}-slim` | mypy |
| Go | `go.mod` | `golang:{version}` | vet |

支持子目录检测：当根目录无法匹配时，自动扫描子目录中的项目。

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

  - name: spotbugs
    image: maven:3.9-eclipse-temurin-17
    commands:
      - mvn spotbugs:check
    depends_on:
      - build

  - name: package
    image: maven:3.9-eclipse-temurin-17
    commands:
      - mvn package -DskipTests
    depends_on:
      - test
```

### 前端（Vue CLI）示例

在 **Vue CLI**（或同类 `package.json` + Vue 启发式命中）的项目根目录执行 **`pipelight init`** 可得到与下面一致的 `pipeline.yml`。流水线在 **`ping-pong` → `git-pull`** 之后为 **类型检查（有 TS 时）→ lint → 单元测试 → 生产构建**；**lockfile / `.nvmrc` 校验合并进第一个会跑 `npm ci` 的步骤**（减少一次容器调度）。`git_credentials` 为占位符，请勿将真实凭据提交到版本库。

```yaml
name: vue-cli-ci
git_credentials:
  username: your_username
  password: your_token_or_password
steps:
- name: ping-pong
  commands:
  - COUNTER_FILE=pipelight-misc/ping-pong-counter; mkdir -p pipelight-misc; COUNT=$(cat "$COUNTER_FILE" 2>/dev/null || echo 0); COUNT=$((COUNT + 1)); echo "$COUNT" > "$COUNTER_FILE"; echo "ping (round $COUNT/10)"; if [ "$COUNT" -ge 10 ]; then rm -f "$COUNTER_FILE"; exit 0; fi; exit 1
  on_failure:
    callback_command: ping
    max_retries: 9
    context_paths: []
    exceptions:
      ping:
        command: ping
        max_retries: 9
  local: true
  active: false
- name: git-pull
  commands:
  - if [ ! -d .git ]; then echo 'Not a git repository, skipping'; exit 0; fi
  - if ! git remote | grep -q .; then echo 'No remote configured, skipping'; exit 0; fi
  - echo "Pulling from $(git remote get-url origin 2>/dev/null || git remote get-url $(git remote | head -1))..."
  - STASHED=false; if ! git diff --quiet || ! git diff --cached --quiet; then echo 'Stashing local changes...'; git stash && STASHED=true; fi
  - 'git pull --rebase || { if $STASHED; then git stash pop; fi; echo ''ERROR: git pull --rebase failed — possible merge conflict''; exit 1; }'
  - 'if $STASHED; then echo ''Restoring stashed changes...''; git stash pop || { echo ''ERROR: stash pop conflict — run git stash pop manually''; exit 1; }; fi'
  depends_on:
  - ping-pong
  on_failure:
    callback_command: git_fail
    max_retries: 0
    context_paths: []
  local: true
- name: lint
  image: node:20-slim
  commands:
  - 'if [ ! -f package-lock.json ]; then echo "ERROR: package-lock.json missing — run npm install and commit the lockfile"; exit 1; fi && if [ -f .nvmrc ]; then WANT=$(grep -v ''^#'' .nvmrc | head -1 | tr -d ''v \r''); MAJOR_WANT=${WANT%%.*}; GOT=$(node -p "process.version.slice(1).split(''.'')[0]"); if [ "$MAJOR_WANT" != "$GOT" ]; then echo "ERROR: Node major mismatch: .nvmrc expects major $MAJOR_WANT, container has $GOT (image must match .nvmrc)"; exit 1; fi; fi && echo "node hygiene: package-lock.json present; .nvmrc matches container (or no .nvmrc)"'
  - npm ci
  - npm run lint
  depends_on:
  - git-pull
  on_failure:
    callback_command: auto_fix
    max_retries: 2
    context_paths:
    - .eslintrc.cjs
    - .eslintrc.js
    - .nvmrc
    - babel.config.js
    - jest.config.js
    - package-lock.json
    - package.json
    - src/
    - tests/
    - vue.config.js
    exceptions:
      hygiene_error:
        command: runtime_error
        max_retries: 0
        context_paths:
        - .eslintrc.cjs
        - .eslintrc.js
        - .nvmrc
        - babel.config.js
        - jest.config.js
        - package-lock.json
        - package.json
        - vue.config.js
      lint_error:
        command: auto_fix
        max_retries: 2
        context_paths:
        - .eslintrc.cjs
        - .eslintrc.js
        - .nvmrc
        - babel.config.js
        - jest.config.js
        - package-lock.json
        - package.json
        - src/
        - tests/
        - vue.config.js
- name: test
  image: node:20-slim
  commands:
  - npm ci
  - CI=true npm run test:unit -- --watchAll=false
  depends_on:
  - lint
  on_failure:
    callback_command: auto_fix
    max_retries: 2
    context_paths:
    - .eslintrc.cjs
    - .eslintrc.js
    - .nvmrc
    - babel.config.js
    - jest.config.js
    - package-lock.json
    - package.json
    - src/
    - tests/
    - vitest.config.js
    - vitest.config.ts
    - vue.config.js
    exceptions:
      test_failure:
        command: auto_fix
        max_retries: 2
        context_paths:
        - .eslintrc.cjs
        - .eslintrc.js
        - .nvmrc
        - babel.config.js
        - jest.config.js
        - package-lock.json
        - package.json
        - src/
        - tests/
        - vitest.config.js
        - vitest.config.ts
        - vue.config.js
- name: build
  image: node:20-slim
  commands:
  - npm ci
  - npm run build
  depends_on:
  - test
  on_failure:
    callback_command: auto_fix
    max_retries: 2
    context_paths:
    - .eslintrc.cjs
    - .eslintrc.js
    - .nvmrc
    - babel.config.js
    - jest.config.js
    - package-lock.json
    - package.json
    - public/
    - src/
    - tests/
    - vite.config.js
    - vite.config.ts
    - vue.config.js
    exceptions:
      compile_error:
        command: auto_fix
        max_retries: 2
        context_paths:
        - .eslintrc.cjs
        - .eslintrc.js
        - .nvmrc
        - babel.config.js
        - jest.config.js
        - package-lock.json
        - package.json
        - public/
        - src/
        - tests/
        - vite.config.js
        - vite.config.ts
        - vue.config.js
```

在 Vue 项目根目录（已生成 `pipeline.yml`）校验并运行：

```bash
cd /path/to/your-vue-app
pipelight validate -f pipeline.yml
pipelight run -f pipeline.yml --dry-run
pipelight run -f pipeline.yml
```

## 运行效果

```
[build] Starting... (maven:3.9-eclipse-temurin-17)
[build] OK (47.4s)
[test] Starting... (maven:3.9-eclipse-temurin-17)
[spotbugs] Starting... (maven:3.9-eclipse-temurin-17)
[test] OK (83.2s)
[spotbugs] OK (12.3s)
[package] Starting... (maven:3.9-eclipse-temurin-17)
[package] OK (30.1s)

Test Summary: 42 passed, 0 failed, 2 skipped

Step             Duration     Status
build            47.4s        OK
test             83.2s        OK
spotbugs         12.3s        OK
package          30.1s        OK
Total            173.0s
```

## 失败处理策略

| 策略 | 行为 |
|------|------|
| `auto_fix` | 退出码 1，保存状态 + 错误日志到 `pipelight-misc/`，等待 LLM agent 修复后 `pipelight retry` |
| `abort` | 退出码 2，终止流水线 |
| `notify` | 退出码 2，通知用户 |

## 架构

```
CLI 层 (clap)        → 子命令: run / init / validate / list / retry / status
    ↓                  顶层 flag: --list-steps
Pipeline 模型层       → YAML → DAG 解析, 变量插值, 条件表达式
    ↓
调度器层             → DAG 拓扑排序, 并行 step 调度 (tokio)
    ↓
执行器层             → Docker API 交互 (bollard)
    ↓
输出层               → 实时日志流, 彩色终端输出 (tty/plain/json)
```

```
src/
  main.rs              → 入口
  cli/                 → clap 命令定义
  run_state/           → 运行状态持久化 (~/.pipelight/runs/)
  ci/                  → CI 流水线核心
    detector/          →   项目类型检测 (策略模式)
    builder/           →   流水线步骤生成 (策略模式)
    parser/            →   Pipeline YAML 解析 & 校验
    scheduler/         →   DAG 构建 & 拓扑排序
    executor/          →   Docker 容器执行
    output/            →   终端日志格式化
```

## 开发

```bash
cargo build          # 编译
cargo test           # 运行测试 (159 单元 + 48 集成)
cargo run -- --help  # 查看帮助
```
