# Plan Wave 2: Lots of Enhancement
- hooks manager
- slash commands manager

## Background

### about slash commands

https://docs.anthropic.com/en/docs/claude-code/slash-commands

> Namespacing
> Organize commands in subdirectories. The subdirectories determine the command’s full name. The description will show whether the command comes from the project directory (.claude/commands) or the user-level directory (~/.claude/commands).
> 
> Conflicts between user and project level commands are not supported. Otherwise, multiple commands with the same base file name can coexist.
> 
> For example, a file at .claude/commands/frontend/component.md creates the command /frontend:component with description showing “(project)”. Meanwhile, a file at ~/.claude/commands/component.md creates the command /component with description showing “(user)”.

- glob the command markdown files when need search and list
- filename is the command name

### about hooks

https://docs.anthropic.com/en/docs/claude-code/hooks

> Claude Code hooks are configured in your settings files:
> 
> ~/.claude/settings.json - User settings
> .claude/settings.json - Project settings
> .claude/settings.local.json - Local project settings (not committed)

```json
{
  "hooks": {
    "PreToolUse|ToolPattern|Notification|Stop|SubagentStop|PreCompact": [
      {
        "matcher": "Task|Bash|Glob|Grep|Read|Edit|MultiEdit|Write|WebFetch|WebSearch", // only applicable for PreToolUse and PostToolUse. * is invalid, instead use "".
        "hooks": [
          {
            "type": "command", // Currently only "command" is supported
            "command": "your-command-here" // The bash command to execute
          }
        ]
      }
    ]
  }
}
```

- no name, display as ${matcher}:${command}

## New subcommands

default scope is current project.

### feature#1 manage slash commands
- `claco commands list --scope=user|project` list all the slash commands
- `claco commands import --scope=user|project $url` import from markdown file from github
- `claco commands clean --scope=user|project` remove all slash commands, need confirm
- `claco commands generate $prompt` generate command from prompt via claude code itself

### feature#2 manage hooks
- `claco hooks list --scope=user|project` list all the slash commands
- `claco hooks add --scope=user|project --event $event --matcher $matcher --command $command`
- `claco hooks del --interactive` list hooks and wait user to pick then delete from config files
