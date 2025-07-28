# claco - Claude Code Helper

![cover](assets/cli-showcase.png)

`claco` (Claude Code Helper) is a CLI tool for boosting Claude Code productive.

## Installation

On Linux/macOS:

- Install via Homebrew(Mac ONLY): `brew install kaichen/tap/claco`
- Install via script `curl -fsSL https://raw.githubusercontent.com/kaichen/claco/main/install.sh | bash`
- Install via crates.io `cargo install claco`
- Install via Github(Unstable) `cargo install --git https://github.com/kaichen/claco`

*NOTICE* cargo is package manager from rust toolchain.

## Features and Usage

- **agents**: Manage custom agents (list, import, delete, clean, generate)
- **commands**: Manage slash commands configurations
- **hooks**: Manage hooks configuration
- **history**: Lists all user input messages for the current project
- **session**: Shows session info including first user message and timestamp
- **projects**: Lists all projects with their session IDs

Manage Custom Sub Agents

```bash
# List all custom agents
claco agents list
# Import agent from GitHub
claco agents import https://github.com/owner/repo/blob/main/agent.md --scope user
# Import agent from local file
claco agents import ../my-agent.md --scope project
# Generate new agent using Claude (or use 'gen' shortcut)
claco agents generate "Create a security analyst agent"
claco agents gen "Create a security analyst agent"
# Generate agent template with all properties
claco agents gen "my-agent" --template
```

Manage Slash Commands

```bash
# List all claude code slash commands
claco commands list
# Import command from github repo
claco commands import https://github.com/amantus-ai/vibetunnel/blob/main/.claude/commands/review-pr.md
# Generate command via claude code cli (or use 'gen' shortcut)
claco commands generate "Checkout yesterday's pull request and generate report"
claco commands gen "Checkout yesterday's pull request and generate report"
# Generate command template with all frontmatter properties
claco commands gen "my-command" --template
```

Manage Hooks

```bash
# List all claude code hooks
claco hooks list
# Add stop sound notification
claco hooks add --scope=user --event=Stop --command="afplay /System/Library/Sounds/Glass.aiff
```

List user messages in current project

```bash
# Show all user messages in the current directory's Claude project
claco history
# Or use the alias
claco showmeyourtalk > dev-prompt.log
# Show messages from a specific session
claco history --session 48fb8f8e-48e9-4eb8-b035-4b72deb386cf >> dev-prompt.log
```

## License

MIT.
