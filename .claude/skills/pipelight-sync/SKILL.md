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
python3 --version     # Python runtime (required by gen_diff_html.py)
python3 -c "import pygments, sys; sys.stdout.write(pygments.__version__)"   # Pygments
```

**For each tool:**
- **Installed + working** → `OK: rustc 1.94.1`
- **Not installed** → auto-install if possible:
  - Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
  - Others: print install instructions
- **Installed but not running** (Docker daemon) → warn user to start Docker

- **python3**:
  - **Installed** → `OK: Python 3.x.y`
  - **Not installed** → print install hint for the platform (`apt install python3` / `brew install python3`); do NOT auto-install (Python is typically pre-installed; auto-install is fragile cross-platform).

- **pygments**:
  - **Installed** → `OK: pygments x.y`
  - **Not installed** → auto-install with the ladder below. Abort the ladder on first success.
    1. `python3 -m pip install --user pygments`
    2. If step 1 fails with "externally-managed-environment" (PEP 668): `python3 -m pip install --user --break-system-packages pygments`
    3. If step 2 also fails: print a red error telling the user to install manually (e.g. `apt install python3-pygments` / `brew install pygments`) and continue the sync — pygments is only blocking when the user later runs `pipelight run --git-diff-from-remote-branch`.

After environment check, check whether a rebuild is needed before compiling.

**Skip-build check:**

The marker file `~/.pipelight/build-commit` stores the git commit hash of the last successful build+install. Check whether any **Rust source or dependency files** changed since that commit:

```bash
MARKER_FILE="$HOME/.pipelight/build-commit"
LAST_BUILD_COMMIT=$(cat "$MARKER_FILE" 2>/dev/null || echo "none")
```

If `LAST_BUILD_COMMIT` is not "none", check for Rust-relevant changes:

```bash
git diff --name-only "$LAST_BUILD_COMMIT"..HEAD -- 'src/' 'Cargo.toml' 'Cargo.lock' 'build.rs'
```

- **If output is empty** (no Rust source/dep changes) AND `pipelight --version` works → skip build+install, report: `pipelight    OK (up to date, skipped build)`
- **If output is non-empty** (Rust files changed) → proceed with build+install below
- **If `LAST_BUILD_COMMIT` is "none"** (first time or marker missing) → always build

**Build (only when needed):**

```bash
cargo build --release 2>&1
```

- **Build succeeds** → proceed to install
- **Build fails** → show error, suggest `cargo clean && cargo build --release`

### Step 2b: Install pipelight binary

After a successful release build, copy the binary to a directory on PATH so it can be invoked as `pipelight` from anywhere.

**Step 1: Detect existing install location**

```bash
EXISTING=$(which pipelight 2>/dev/null || true)
```

If `EXISTING` is non-empty, the binary is already installed somewhere on PATH. **Always install to that same path** to avoid PATH-priority conflicts (e.g. `~/.local/bin` shadowing `~/.cargo/bin`).

**Step 2: Install**

```bash
BINARY=target/release/pipelight

if [ -n "$EXISTING" ]; then
  # Overwrite existing location — try direct copy, fall back to sudo
  cp "$BINARY" "$EXISTING" 2>/dev/null || sudo cp "$BINARY" "$EXISTING"
  INSTALL_PATH="$EXISTING"
else
  # Fresh install — pick a directory on PATH
  # Priority: ~/.local/bin (user-local, no sudo) > ~/.cargo/bin > /usr/local/bin
  if [ -d "$HOME/.local/bin" ]; then
    cp "$BINARY" "$HOME/.local/bin/pipelight"
    INSTALL_PATH="$HOME/.local/bin/pipelight"
  elif [ -d "$HOME/.cargo/bin" ]; then
    cp "$BINARY" "$HOME/.cargo/bin/pipelight"
    INSTALL_PATH="$HOME/.cargo/bin/pipelight"
  else
    # /usr/local/bin as last resort (may need sudo on Linux)
    cp "$BINARY" /usr/local/bin/pipelight 2>/dev/null \
      || sudo cp "$BINARY" /usr/local/bin/pipelight
    INSTALL_PATH="/usr/local/bin/pipelight"
  fi
fi
```

**Write marker after successful install:**

```bash
mkdir -p ~/.pipelight
git rev-parse HEAD > ~/.pipelight/build-commit
```

Note: also update the marker when build is skipped (so future diffs start from the latest HEAD):

```bash
git rev-parse HEAD > ~/.pipelight/build-commit
```

**Verify installation:**

```bash
pipelight --version
which pipelight
```

- **Works** → `OK: pipelight 0.1.0 (installed to $INSTALL_PATH)`
- **Not found** → warn user to add `~/.local/bin` or `~/.cargo/bin` to PATH

### Step 2b2: Run Unit Tests

After a successful build (or when build was skipped but Rust source changed since last test), run all unit and integration tests:

```bash
cargo test 2>&1
```

- **All passed** → `tests    OK (N passed)`
- **Some failed** → show failure details, do NOT proceed to install. Report the failures and stop.

If build was skipped and no Rust source changed, skip tests too.

### Step 2c: Install Global Skills

Install skills from the repo's `global-skills/` directory to `~/.claude/skills/` so they are available in all projects.

```bash
# List all skills in global-skills/
ls global-skills/
```

For each subdirectory in `global-skills/`:

```bash
# Remove any existing install first to avoid nested dirs on reinstall, then copy fresh
rm -rf ~/.claude/skills/<skill-name>
cp -r global-skills/<skill-name> ~/.claude/skills/<skill-name>
```

**Example:**

```bash
cp -r global-skills/pipelight-run ~/.claude/skills/pipelight-run
```

Report what was installed:

```
Global skills:
  pipelight-run   OK (installed to ~/.claude/skills/pipelight-run)
```

If `global-skills/` directory doesn't exist, skip this step silently.

### Step 2c2: Sync pipelight-run skill to Trae

If Trae is installed locally, also sync `pipelight-run` into Trae's global skill directory so the same callback protocol works there.

**Detect Trae:**

```bash
[ -d "$HOME/.trae" ]
```

- **Not installed** (no `~/.trae` dir) → skip this step silently, report `trae   SKIPPED (not installed)`
- **Installed** → sync:

```bash
mkdir -p ~/.trae/skills
rm -rf ~/.trae/skills/pipelight-run
cp -r global-skills/pipelight-run ~/.trae/skills/pipelight-run
```

Report:

```
Trae skills:
  pipelight-run   OK (installed to ~/.trae/skills/pipelight-run)
```

### Step 3: Load Skill Memory

Skill-memory files now ship **inside each global skill** under `global-skills/<skill>/memory/*.md`, so Step 2c already deployed them to `~/.claude/skills/<skill>/memory/`. The owning skill is responsible for re-reading its own `memory/` on every invocation (see e.g. pipelight-run's Step 0).

For this sync session, still read every `*.md` under every `global-skills/*/memory/` directory so the rules shape behavior immediately — don't wait for the next skill invocation.

```bash
ls global-skills/*/memory/*.md 2>/dev/null
```

For each file:
1. Read it in full.
2. Treat its content with the same authority as an auto-memory entry in `~/.claude/projects/*/memory/`.
3. If the frontmatter `type` is `feedback`, obey the **Why / How to apply** sections when the trigger condition arises.

**Do NOT read `docs/`** during sync — project-docs are reference material, not behavior rules.

Output a brief summary listing each memory file loaded:

```
Skill memory loaded:
- pipelight-run/memory/feedback_pipelight_callback_dispatch.md — pipelight JSON: 逐 step dispatch on_failure, 按 action 完整执行
- <other-skill>/memory/<file>.md — <one-line hook from description field>
```

If no memory files exist, skip this step silently.

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
  python3      OK 3.x.y
  pygments     OK x.y   — or INSTALLED (via pip --user) — or FAILED (manual install required)

Build:
  cargo build  OK (release, N warnings)  — or SKIPPED (no code changes since last build)
  cargo test   OK (N passed)  — or SKIPPED (no code changes)
  pipelight    OK installed (/usr/local/bin/pipelight or ~/.cargo/bin/pipelight)  — or OK (up to date, skipped build)

Global Skills:
  pipelight-run  OK (installed to ~/.claude/skills/)

Trae Skills:
  pipelight-run  OK (installed to ~/.trae/skills/)  — or SKIPPED (trae not installed)

Skill Memory:
  global-skills/*/memory/  OK (N memory files loaded)

Ready to develop!
```

If anything failed, end with actionable next steps instead of "Ready to develop!".
