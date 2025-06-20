# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-06-20

### Added
- Command alias `showmeyourtalk` for the `history` command
- Slash command detection in history output (shows `/command` instead of full XML)
- Skip next line after slash commands in history output
- Automatic detection of most recent session when no session ID provided to `session` command
- Better error handling for complex message formats
- Filtering for sidechain messages (only shows `isSidechain:false`)
- Skip caveat messages in history output

### Changed
- Improved path handling by reading actual `cwd` from JSONL files instead of reversing sanitization
- Better filtering for user messages (requires both `type:user` and `role:user`)
- More robust JSON parsing with graceful error handling
- Cleaner output formatting

### Fixed
- Fixed "No Claude project found" error when project exists but path sanitization doesn't match
- Fixed JSON deserialization errors for complex message content
- Fixed incorrect path reconstruction for projects with special characters or multiple slashes

## [0.1.0] - 2025-06-19

### Added
- Initial implementation of claco (Claude Code CLI Inspector)
- `history` command to list all user input messages in current project
  - Support for filtering by specific session ID
  - Displays timestamp and message content
- `session` command to display session information by ID
  - Shows session ID, project path, start time, and first user message
- `projects` command to list all projects with their sessions
  - Shows project folder path and array of session IDs
- `live` command to list all active Claude sessions
  - Displays PID, IDE name, and workspace folders
- Basic CLI structure with clap for argument parsing
- Logging support with tracing
- Configuration system (though not actively used yet)

### Technical Details
- Reads from `~/.claude` directory structure
- Handles JSONL format for session logs
- Processes lock files for active sessions
- Path sanitization/desanitization for project directories

[0.2.0]: https://github.com/kaichen/claco/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kaichen/claco/releases/tag/v0.1.0