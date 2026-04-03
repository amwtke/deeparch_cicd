# Architecture

## Current Layers

```
CLI (clap)               → subcommands: run / validate / list / logs
    ↓
Pipeline Model (serde)   → YAML → DAG parse, variable interpolation, conditions
    ↓
Scheduler (petgraph)     → DAG topological sort, parallel step scheduling (tokio)
    ↓
Executor (bollard)       → Docker container lifecycle management
    ↓
Output (console)         → Real-time colored terminal output
```

## Planned: LLM Executor Layer

```
Executor Layer
    ├── DockerExecutor      ← existing: run commands in Docker containers
    └── ClaudeExecutor      ← planned: invoke Claude Code CLI as sub-agent
```

### ClaudeExecutor design

Invocation method: spawn `claude` CLI process in headless mode (`-p` flag).

```
pipelight detects step failure
    → collects error log + relevant source code
    → spawns: claude -p "Fix this build error: {context}" --allowedTools Edit,Bash
    → Claude Code reads/edits files, runs commands
    → pipelight re-runs the failed step
    → repeat up to max_retries
```

Billing: uses existing Claude Code subscription (Max plan or API key), no extra cost.

### MCP Server (future)

Expose pipelight as MCP tools to Claude Code:

- `pipelight.run` — trigger a pipeline
- `pipelight.status` — check pipeline status  
- `pipelight.logs` — get step logs

This enables Claude Code to natively trigger and monitor CI/CD within a conversation.

## Directory Structure

```
src/
  main.rs              → entry point
  cli/mod.rs           → clap command definitions
  pipeline/mod.rs      → YAML parsing & Pipeline data model
  scheduler/mod.rs     → DAG construction & topological sort scheduling
  executor/mod.rs      → Docker container executor
  output/mod.rs        → terminal log output formatting
```

## Key Crates

| Crate | Purpose |
|-------|---------|
| clap | CLI argument parsing |
| tokio | async runtime |
| serde + serde_yaml | YAML config parsing |
| bollard | Docker API (no shelling out to docker CLI) |
| petgraph | DAG construction and scheduling |
| anyhow + thiserror | error handling |
| tracing | structured logging |
| futures-util | stream utilities for Docker log streaming |
