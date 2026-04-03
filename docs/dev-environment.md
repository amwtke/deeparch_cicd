# Development Environment

## Machines

| Machine | OS | CPU | Architecture |
|---------|-----|-----|-------------|
| Mac M1 | macOS | Apple Silicon | aarch64 |
| Mac Intel | macOS | Intel | x86_64 |
| Ubuntu | Ubuntu Linux | Intel | x86_64 |

## Required Tools

| Tool | Purpose | Install |
|------|---------|---------|
| Rust (rustup + cargo) | Build & run | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Docker | Step execution | platform-specific (Docker Desktop on macOS, docker-ce on Ubuntu) |
| Git | Version control | pre-installed on macOS, `apt install git` on Ubuntu |
| Claude Code | AI integration | `npm install -g @anthropic-ai/claude-code` or Homebrew |

## Development Workflow

1. Each machine builds natively — no cross-compilation needed for development
2. `cargo build` works identically on all three machines
3. Code syncs via Git (`git@github.com:amwtke/deeparch_cicd.git`, branch `main`)
4. `Cargo.lock` is committed to ensure dependency version consistency

## Environment Check

To verify a machine is ready:

```bash
rustc --version     # Rust compiler
cargo --version     # Build tool
docker --version    # Container runtime
git --version       # Version control
claude --version    # Claude Code CLI (optional, for LLM features)
```

## Decisions

- **No cross-compilation for dev** — each machine builds its own native binary, simple and reliable
- **Cross-compile only for release** — use `cargo-zigbuild` if needed to produce Linux binaries from macOS
- **Docker required** — all pipeline steps run in containers, Docker daemon must be running
- **Pure Rust dependencies** — no C library dependencies, simplifies multi-platform builds
