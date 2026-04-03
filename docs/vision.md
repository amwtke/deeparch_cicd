# Pipelight Vision

## What is Pipelight

A lightweight CLI CI/CD tool built in Rust. Pipelines are defined in YAML, steps execute in Docker containers with DAG-based parallel scheduling.

## Why Pipelight exists

Traditional CI/CD tools (Jenkins, GitLab CI, GitHub Actions) are heavyweight, cloud-dependent, and mechanically execute predefined commands. They have no understanding of what they're running.

Pipelight is different:

1. **Local-first** — runs on your machine, no server needed, works offline
2. **CLI-native** — designed for terminal workflows, not web UIs
3. **AI-native** — integrates LLM (Claude) into the pipeline process, not as an afterthought

## Core Differentiator: LLM Integration

Traditional CI/CD:
```
build fails → dump error log → human reads → human fixes → re-trigger
```

Pipelight:
```
build fails → Claude analyzes error → Claude fixes code → re-build → pass
```

### Three levels of LLM integration

| Level | How | Use case |
|-------|-----|----------|
| Claude Code CLI | `pipelight` spawns `claude -p` subprocess | Auto-fix failures, generate code |
| MCP Server | `pipelight` exposes tools to Claude Code | Claude Code triggers pipelines natively |
| Claude API | Direct HTTP to api.anthropic.com | Log analysis, code review (separate billing) |

**Priority: Claude Code CLI first** — already available, no extra cost, full agent capabilities.

## What Pipelight is NOT

- Not a Jenkins replacement with a web UI
- Not a cloud CI/CD service
- Not trying to support every language/platform — Rust-first, opinionated
