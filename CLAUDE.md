# Pipelight - 轻量级 CLI CI/CD 工具

## 项目概述
一个用 Rust 实现的轻量级 CI/CD CLI 工具，通过 Docker 容器隔离执行流水线步骤。
目标：替代 Jenkins/GitLab CI 等重量级方案，提供本地优先、快速、可离线使用的 CI/CD 体验。

## 架构分层
```
CLI 层 (clap)        → 子命令: run / validate / list / logs
    ↓
Pipeline 模型层       → YAML → DAG 解析, 变量插值, 条件表达式
    ↓
调度器层             → DAG 拓扑排序, 并行 step 调度 (tokio)
    ↓
执行器层             → Docker API 交互 (bollard)
    ↓
输出层               → 实时日志流, 彩色终端输出
```

## 编码约定
- 错误处理统一用 `anyhow::Result` (应用层) + `thiserror` (库层自定义错误)
- 所有异步操作基于 `tokio`
- 日志统一用 `tracing` (不要用 println! 或 log crate)
- Docker 交互统一通过 `bollard` crate，不要 shell 调用 docker CLI
- YAML 解析用 `serde` + `serde_yaml`
- DAG 用 `petgraph` 构建和调度
- 代码注释用英文，用户面向的输出用英文

## 关键设计决策
- Pipeline 配置文件默认名: `pipeline.yml`
- Step 间通过共享 Docker volume 传递 artifact
- 环境变量支持 `${VAR}` 插值语法
- 并行执行无依赖关系的 step

## 目录结构
```
src/
  main.rs                    → 入口
  cli/mod.rs                  → clap 命令定义
  pipeline/mod.rs             → YAML 解析 & Pipeline 数据模型
  scheduler/mod.rs            → DAG 构建 & 拓扑排序调度
  executor/mod.rs             → Docker 容器执行器
  output/mod.rs               → 终端日志输出格式化
  detector/                   → 项目类型检测 (策略模式)
    base/mod.rs               → ProjectDetector trait, ProjectInfo, ProjectType, 检测编排
    maven.rs / gradle.rs / …  → 各语言检测策略实现
  pipeline_gen/               → 流水线生成 (策略模式)
    base/mod.rs               → BaseStrategy 共享 step 构建器
    maven/ / rust_lang/ / …   → 各语言流水线步骤策略实现
```
