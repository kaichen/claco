# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`claco` (Claude Code Helper) is a CLI tool for boosting Claude Code productivity by managing custom agents, slash commands, hooks, and inspecting Claude Code sessions.

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
- Run `cargo lint` after edit or write any files
- No emoji for console print, if needed use special ascii characters

## Architecture

The project is a Rust CLI application with the following structure:

- **main.rs**: Entry point that handles input/output and orchestrates the application flow
- **cli.rs**: Defines CLI arguments using clap's derive API
- **config.rs**: Manages configuration loading/saving using platform-specific directories
- **lib.rs**: Library exports for the public API

### Current Implementation

**Implemented Features**:
1. `history` command: List all user messages for a project
2. `session` command: Display session info by ID
3. `projects` command: List all projects with their sessions
4. `agents` subcommands: Manage custom agents (list, import, generate, delete, clean)
5. `commands` subcommands: Manage slash commands (list, import, generate, delete, clean)
6. `hooks` subcommands: Manage hooks for Claude Code events (list, add, delete)

**Architecture Improvements**:
- Robust error handling with Result types throughout
- Atomic file operations to prevent corruption
- Protection against path traversal attacks
- Proper handling of concurrent access

### Claude Code Data Structure

Claude Code stores data in `~/.claude/`:
- **ide/**: Live session lock files (e.g., `34946.lock`)
- **projects/**: JSONL logs for each project session
- **agents/**: Custom agent markdown files
- **commands/**: Custom slash command markdown files
- **settings.json**: User-level settings including hooks

Project-specific data in `.claude/`:
- **agents/**: Project-specific custom agents
- **commands/**: Project-specific slash commands
- **settings.json**: Project-level settings including hooks

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

## claude command help

```
Usage: claude [options] [command] [prompt]

Claude Code - starts an interactive session by default, use -p/--print for non-interactive output

Arguments:
  prompt                           Your prompt

Options:
  -d, --debug                      Enable debug mode
  --verbose                        Override verbose mode setting from config
  -p, --print                      Print response and exit (useful for pipes)
  --output-format <format>         Output format (only works with --print): "text" (default), "json" (single result), or "stream-json" (realtime streaming) (choices: "text", "json",
                                   "stream-json")
  --input-format <format>          Input format (only works with --print): "text" (default), or "stream-json" (realtime streaming input) (choices: "text", "stream-json")
  --mcp-debug                      [DEPRECATED. Use --debug instead] Enable MCP debug mode (shows MCP server errors)
  --dangerously-skip-permissions   Bypass all permission checks. Recommended only for sandboxes with no internet access.
  --allowedTools <tools...>        Comma or space-separated list of tool names to allow (e.g. "Bash(git:*) Edit")
  --disallowedTools <tools...>     Comma or space-separated list of tool names to deny (e.g. "Bash(git:*) Edit")
  --mcp-config <file or string>    Load MCP servers from a JSON file or string
  --append-system-prompt <prompt>  Append a system prompt to the default system prompt
  --permission-mode <mode>         Permission mode to use for the session (choices: "acceptEdits", "bypassPermissions", "default", "plan")
  -c, --continue                   Continue the most recent conversation
  -r, --resume [sessionId]         Resume a conversation - provide a session ID or interactively select a conversation to resume
  --model <model>                  Model for the current session. Provide an alias for the latest model (e.g. 'sonnet' or 'opus') or a model's full name (e.g.
                                   'claude-sonnet-4-20250514').
  --fallback-model <model>         Enable automatic fallback to specified model when default model is overloaded (only works with --print)
  --settings <file>                Path to a settings JSON file to load additional settings from
  --add-dir <directories...>       Additional directories to allow tool access to
  --ide                            Automatically connect to IDE on startup if exactly one valid IDE is available
  --strict-mcp-config              Only use MCP servers from --mcp-config, ignoring all other MCP configurations
  --session-id <uuid>              Use a specific session ID for the conversation (must be a valid UUID)
  -v, --version                    Output the version number
  -h, --help                       Display help for command
```

## Implementation Details

### Custom Agents
Agents are markdown files with YAML frontmatter:
```yaml
---
name: agent-name
description: Brief description
color: blue  # optional
tools: [Tool1, Tool2]  # optional
---

# Agent Instructions
...
```

### Slash Commands
Commands are markdown files with optional YAML frontmatter:
```yaml
---
description: Command description
allowed-tools: Tool1, Tool2, Tool3
---

Command content here...
```

### Hooks
Hooks are stored in settings.json and support various events:
- `tool:*` - All tool events
- `tool:bash` - Specific tool events
- Custom matchers for fine-grained control

### Error Handling
- All functions that interact with the filesystem return `Result<T>`
- The `claude_home()` function returns `Result<PathBuf>` to handle missing home directory
- Atomic file operations ensure data integrity
