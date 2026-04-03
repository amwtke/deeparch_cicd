# Technical Decisions Log

## 2026-04-03: Project initialization

### D001: LLM integration via Claude Code CLI first
- **Decision**: Use Claude Code CLI (`claude -p`) as the first LLM integration method, not Claude API
- **Reason**: No extra cost (uses existing subscription), full agent capabilities (file editing, command execution), simpler implementation (just spawn a process)
- **Alternative rejected**: Claude API — separate billing, requires implementing tool use ourselves

### D002: No cross-compilation for development
- **Decision**: Each machine (Mac M1, Mac Intel, Ubuntu) builds natively with `cargo build`
- **Reason**: Pure Rust project, no C dependencies, cross-compilation adds unnecessary complexity
- **When to revisit**: If C dependencies are added, or for release binary distribution

### D003: Shared knowledge in docs/ not local memory
- **Decision**: Project knowledge and architecture decisions live in `docs/` committed to Git, not in Claude Code local memory
- **Reason**: Three development machines need the same context. Local `.claude/memory/` is per-machine and not shared.

### D004: Docker as isolation layer
- **Decision**: All pipeline steps execute in Docker containers via bollard crate
- **Reason**: Environment consistency, isolation between steps, artifact sharing via volumes
- **Constraint**: Docker daemon must be running on dev machine
