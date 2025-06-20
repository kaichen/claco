# claco - Claude Code CLI Inspector

![cover](assets/cover-show-me-your-talk.png)

`claco` (Claude Code Inspector) is a CLI tool for inspecting Claude Code sessions and project data stored in the `~/.claude` directory.

If you're a Claude Code user, you can use this claco tool to quickly print out all the commands you've issued in the current project with just one click.

## Installation

### From source
```bash
cargo install --path .
```

### From crates.io (when published)
```bash
cargo install claco
```

### From GitHub
```bash
cargo install --git https://github.com/kaichen/claco
```

## Usage

### List user messages in current project
```bash
# Show all user messages in the current directory's Claude project
claco history
# Or use the alias
claco showmeyourtalk

# Show messages from a specific session
claco history --session 48fb8f8e-48e9-4eb8-b035-4b72deb386cf
```

### Show session information
```bash
# Display info about the most recent session
claco session

# Display info about a specific session by ID
claco session 48fb8f8e-48e9-4eb8-b035-4b72deb386cf
```

### List all projects
```bash
# List all Claude projects and their sessions
claco projects
```

### Show active sessions
```bash
# List all currently active Claude sessions
claco live
```

## Features

- **history**: Lists all user input messages for the current project
- **session**: Displays session info including first user message and timestamp
- **projects**: Lists all projects with their session IDs
- **live**: Shows active Claude sessions with PID and workspace info

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Check code
cargo clippy

# Format code
cargo fmt
```