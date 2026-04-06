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

## 视频生成 Skills

本项目集成了一系列 Claude Code skill，可一键生成技术讲解视频（md → vmd → mp4）。

### 完整视频生成（输出 MP4）

#### 深度技术类

| Skill | 命令 | 说明 |
|-------|------|------|
| deeparch-video-gen | `/deeparch-video-gen <主题>` | 单主题技术视频全流程 |
| kernel-trace-gen-video | `/kernel-trace-gen-video <tag>` | 从 xiaojin-* 内核注释标签生成视频 |
| deeparch-html-video-gen | `/deeparch-html-video-gen <html_path>` | HTML 可视化文件 → 裁切截图 → 视频 |
| socratic-video-gen | `/socratic-video-gen <问题>` | 苏格拉底式双人对话视频 + 上传B站 |

#### 新闻播报类

| Skill | 命令 | 说明 |
|-------|------|------|
| ai-news-china-video-gen | `/ai-news-china-video-gen` | 国内 AI 新闻播报视频 + 上传B站 |
| ai-news-global-video-gen | `/ai-news-global-video-gen` | 全球 AI 新闻播报视频 + 上传B站 |

#### 批量生产线类

| Skill | 命令 | 说明 |
|-------|------|------|
| daily-video-gen | `/daily-video-gen` | 从 seeds/ 取课题 + 抓热点，批量生成，支持断点续作 |
| online-daily-gen | `/online-daily-gen` | 实时搜索 7 大技术领域热点，批量生成 |
| topic-gen-video | `/topic-gen-video <方向> [数量]` | 生成整个系列：选题 → 文档 → 脚本 → MP4 |

#### 短视频类

| Skill | 命令 | 说明 |
|-------|------|------|
| short-video-gen | `/short-video-gen <话题\|文件>` | 抖音短视频（60-90秒竖屏黑金风格） |

### 仅生成脚本（输出 .vmd）

| Skill | 命令 | 说明 |
|-------|------|------|
| deeparch-video-script | `/deeparch-video-script <主题>` | 技术视频双轨脚本（画面轨 + 旁白轨） |
| kernel-trace-vmd | `/kernel-trace-vmd <文件路径\|标签名>` | 内核分析文档 → SCQA 视频脚本 |
| tech-news-video-script | `/tech-news-video-script <md路径\|主题>` | 新闻播报风格视频脚本 |

### 参数说明

#### deeparch-video-gen

```bash
/deeparch-video-gen <主题> [lang=zh|en|zh,en] [--short] [--long] [--full] [--open-elec] [--dtime <时间>] [--fast-encode]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<主题>` | 是 | 技术主题，如 "BeanDefinition 的对象蓝图机制" |
| `lang=xx` | 否 | 语言：zh（默认）、en、zh,en（多语言） |
| `--short` | 否 | 精简模式，≤5 分钟，不做源码分析 |
| `--long` | 否 | 深度长视频，30-60 分钟，苏格拉底递归 |
| `--full` | 否 | 完整模式，生成 3 个视频（分析 + 源码 + 实操） |
| `--open-elec` | 否 | 开启B站充电功能 |
| `--dtime <时间>` | 否 | 定时发布：`+5h`、`2026-03-21 18:00`、Unix 时间戳 |
| `--fast-encode` | 否 | 硬件加速编码 |

#### kernel-trace-gen-video

```bash
/kernel-trace-gen-video <tag> [cover=封面文字] [--open-elec] [--dtime <时间>] [--fast-encode]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<tag>` | 是 | xiaojin-* 内核标签，如 `xiaojin-spinlock` 或简写 `spinlock` |
| `cover=封面文字` | 否 | 视频封面文字（不指定则自动生成） |

#### deeparch-html-video-gen

```bash
/deeparch-html-video-gen <html_path> [--fast-encode] [--dtime <时间>] [--open-elec]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<html_path>` | 是 | HTML 可视化文件路径 |

#### socratic-video-gen

```bash
/socratic-video-gen <问题> [lang=zh|en|zh,en] [--fast-encode] [--dtime <时间>]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<问题>` | 是 | 技术问题，如 "Redis为什么这么快" |
| `lang=xx` | 否 | 语言：zh（默认）、en、zh,en |

#### ai-news-china-video-gen / ai-news-global-video-gen

```bash
/ai-news-china-video-gen [--open-elec] [--dtime <时间>] [--fast-encode]
/ai-news-global-video-gen [--open-elec] [--dtime <时间>] [--fast-encode]
```

无必填参数，直接运行即可。

#### daily-video-gen

```bash
/daily-video-gen [--dry-run] [--only-seeds] [--only-hot] [--skip <种子文件>] [--count <N>] [--short] [--open-elec] [--dtime <时间>] [--dtime-interval <秒>] [--fast-encode] [lang=zh|en]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--dry-run` | 否 | 只列出课题，不生成 |
| `--only-seeds` | 否 | 仅用种子文件，跳过网络热点 |
| `--only-hot` | 否 | 仅抓网络热点，跳过种子 |
| `--skip <文件名>` | 否 | 跳过指定种子文件（可多次使用） |
| `--count <N>` | 否 | 每个种子文件取 N 个课题（默认 1） |
| `--dtime-interval <秒>` | 否 | 定时发布间隔（默认 3600 = 1 小时） |

#### online-daily-gen

```bash
/online-daily-gen [--dry-run] [--count <N>] [--only <领域>] [--skip <领域>] [--short] [--open-elec] [--dtime <时间>] [--dtime-interval <秒>] [--fast-encode] [lang=zh|en|zh,en]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--dry-run` | 否 | 只列出课题，不生成 |
| `--count <N>` | 否 | 每个领域取 N 个课题（默认 1，按权重） |
| `--only <领域>` | 否 | 仅搜索指定领域（逗号分隔：`spring,jvm`） |
| `--skip <领域>` | 否 | 跳过指定领域 |

#### topic-gen-video

```bash
/topic-gen-video <主题方向> [选题数量] [--title <系列前缀>] [--short] [--open-elec] [--dtime <时间>] [--fast-encode] [lang=zh|en|zh,en]
# 或从已有选题文件继续：
/topic-gen-video topics/<path>/topics.md
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<主题方向>` | 是 | 技术方向，如 `sched_ext`、`eBPF` |
| `[选题数量]` | 否 | 生成几期（默认 5） |
| `--title <文字>` | 否 | 系列前缀名（跳过自动生成和验证） |

#### short-video-gen

```bash
/short-video-gen <话题|file.md|file.vmd> [--bgm <path>] [--no-render] [--no-tts]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `<输入>` | 是 | 话题文字、.md 文件路径、或 .vmd 文件路径 |
| `--bgm <path>` | 否 | 背景音乐文件路径 |
| `--no-render` | 否 | 只生成 .vmd，不渲染 |
| `--no-tts` | 否 | 静音预览模式 |

#### deeparch-video-script

```bash
/deeparch-video-script <主题> [cover=封面文字] [--short]
```

#### kernel-trace-vmd

```bash
/kernel-trace-vmd <trace_*.md文件路径|标签名> [cover=封面文字]
```

#### tech-news-video-script

```bash
/tech-news-video-script <.md文件路径|新闻主题>
```

### 通用参数

以下参数在大多数视频生成 skill 中通用：

| 参数 | 说明 |
|------|------|
| `--open-elec` | 开启B站充电功能 |
| `--dtime <时间>` | 定时发布，支持 `+5h`、`+1d`、`2026-03-21 18:00`、Unix 时间戳 |
| `--fast-encode` | 硬件加速编码（优先速度） |
| `lang=zh\|en\|zh,en` | 输出语言（默认 zh） |
| `--short` | 精简模式（≤5 分钟） |

## 开发

```bash
cargo build          # 编译
cargo test           # 运行测试
cargo run -- --help  # 查看帮助
```
