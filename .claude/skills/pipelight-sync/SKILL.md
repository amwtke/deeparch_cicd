---
name: pipelight-sync
description: Switch to a new machine — sync repo, check dev environment, restore project context. Use when starting work on a different machine or resuming after a break.
---

# /pipelight-sync

## Trigger

User types `/pipelight-sync`.

## Purpose

One command to get any machine ready for pipelight development. Handles three concerns:
1. Code sync (git pull/push)
2. Dev environment verification (Rust, Docker)
3. Knowledge base sync (read docs/ to restore context)

## Execution Flow

### Step 1: Sync Git Repository

First, check for uncommitted local changes:

```bash
git status
```

**If there are uncommitted changes:**
- Stage and commit them with message `chore: sync local changes before pull`
- Then pull

**If clean:**
- Just pull

```bash
git pull --rebase origin main
```

Then check for unpushed local commits:

```bash
git log origin/main..HEAD --oneline
```

If there are unpushed commits, push them:

```bash
git push
```

Report what happened (files updated, commits pushed, already up to date, etc.)

### Step 2: Check Dev Environment

Run these checks and report status:

```bash
rustc --version      # Rust compiler
cargo --version      # Build tool  
docker --version     # Container runtime
docker info          # Docker daemon running?
git --version        # Git
claude --version     # Claude Code CLI (optional)
```

**For each tool:**
- **Installed + working** → `OK: rustc 1.94.1`
- **Not installed** → auto-install if possible:
  - Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
  - Others: print install instructions
- **Installed but not running** (Docker daemon) → warn user to start Docker

After environment check, build the release binary and install it:

```bash
cargo build --release 2>&1
```

- **Build succeeds** → proceed to install
- **Build fails** → show error, suggest `cargo clean && cargo build --release`

### Step 2b: Install pipelight binary

After a successful release build, copy the binary to a directory on PATH so it can be invoked as `pipelight` from anywhere.

**macOS:**

```bash
# Prefer /usr/local/bin (no sudo needed on most macOS setups)
cp target/release/pipelight /usr/local/bin/pipelight
```

If `/usr/local/bin` is not writable, fall back to `~/.cargo/bin/` (which is already on PATH if Rust is installed via rustup):

```bash
cp target/release/pipelight ~/.cargo/bin/pipelight
```

**Linux:**

```bash
# Try /usr/local/bin first (may need sudo)
sudo cp target/release/pipelight /usr/local/bin/pipelight 2>/dev/null \
  || cp target/release/pipelight ~/.cargo/bin/pipelight
```

**Verify installation:**

```bash
pipelight --version
```

- **Works** → `OK: pipelight 0.1.0 (installed to /usr/local/bin/pipelight)`
- **Not found** → warn user to add `~/.cargo/bin` to PATH

### Step 3: Sync Knowledge Base

Read the shared knowledge base to restore project context:

1. Read `docs/README.md` — index of all docs
2. Read `docs/vision.md` — project vision and AI-native philosophy
3. Read `docs/architecture.md` — current architecture and planned changes
4. Read `docs/decisions.md` — technical decisions log
5. Read `docs/dev-environment.md` — multi-machine setup details

After reading, output a brief summary:

```
Knowledge base loaded:
- Vision: AI-native CLI CI/CD tool with Claude integration
- Architecture: 5 layers, ClaudeExecutor planned
- Decisions: N decisions recorded, latest: [title]
- Environment: 3 machines (Mac M1 / Mac Intel / Ubuntu)
```

### Step 4: Report

Output a summary table:

```
=== Pipelight Sync Complete ===

Git:
  repo         OK (up to date / N files updated / N commits pushed)

Environment:
  rustc        OK 1.94.1
  cargo        OK 1.94.1
  docker       OK 27.x.x (daemon running)
  git          OK 2.x.x
  claude       OK 1.x.x (optional)

Build:
  cargo build  OK (release, N warnings)
  pipelight    OK installed (/usr/local/bin/pipelight or ~/.cargo/bin/pipelight)

Knowledge:
  docs/        OK (N documents loaded)

Ready to develop!
```

If anything failed, end with actionable next steps instead of "Ready to develop!".
