# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`claco` (Claude Code Inspector) is a CLI tool for inspecting Claude Code sessions and project data stored in the `~/.claude` directory.

## Build and Development Commands

```bash
# Build the project
cargo build

# Run the project
cargo run -- [OPTIONS] [INPUT]

# Run tests
cargo test

# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Development Best Practices

- Always run `cargo fmt` before git commit to ensure consistent code formatting

## Architecture

The project is a Rust CLI application with the following structure:

- **main.rs**: Entry point that handles input/output and orchestrates the application flow
- **cli.rs**: Defines CLI arguments using clap's derive API
- **config.rs**: Manages configuration loading/saving using platform-specific directories
- **lib.rs**: Library exports for the public API

### Current vs Planned Implementation

**Current State**: The codebase has basic CLI scaffolding but does not implement the planned Claude inspection features.

**Planned Features** (from context/plan.md):
1. `history` command: List all user messages for a project
2. `session` command: Display session info by ID
3. `projects` command: List all projects with their sessions
4. `live` command: List active Claude sessions

### Claude Code Data Structure

Claude Code stores data in `~/.claude/`:
- **ide/**: Live session lock files (e.g., `34946.lock`)
- **projects/**: JSONL logs for each project session

Project directories are named by sanitizing the working directory path:
```
/Users/kaichen/workspace/claco → Users-kaichen-workspace-claco
```

Each JSONL entry contains:
- `sessionId`: UUID matching the filename
- `message`: The actual message content
- `timestamp`: ISO timestamp
- `type`: "user", "assistant", or "system"
- `cwd`: Working directory
- Additional metadata (parentUuid, version, etc.)

## Implementation Notes

When implementing the planned features, you'll need to:
1. Replace the current generic data processing logic in `main.rs`
2. Add command parsing for history/session/projects/live subcommands
3. Implement JSONL parsing for session files
4. Handle platform-specific home directory paths
5. Parse the sanitized project directory names back to original paths
```