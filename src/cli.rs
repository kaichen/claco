use clap::{Parser, Subcommand};

/// `claco` (Claude Code Helper) is a CLI tool for boosting Claude Code productive.
#[derive(Parser)]
#[command(name = "claco")]
#[command(author, version, about = "`claco` (Claude Code Helper) is a CLI tool for boosting Claude Code productive.", long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage custom agents
    #[command(subcommand)]
    Agents(AgentsSubcommand),
    /// Manage slash commands
    #[command(subcommand)]
    Commands(CommandsSubcommand),
    /// Manage hooks
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// List all user input messages for the current project
    #[command(alias = "showmeyourtalk")]
    History {
        /// Show messages from a specific session ID
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Display session info by ID (defaults to most recent session)
    Session {
        /// Session ID to display (if not provided, shows most recent session)
        session_id: Option<String>,
    },
    /// List all projects with their sessions
    Projects,
    /// Manage Claude Code settings
    #[command(subcommand)]
    Settings(SettingsSubcommand),
}

#[derive(Subcommand)]
pub enum HooksAction {
    /// List all hooks
    List {
        /// Scope to list hooks from (user or project, defaults to showing both)
        #[arg(long)]
        scope: Option<String>,
    },
    /// Add a new hook
    Add {
        /// Scope to add hook to (user or project)
        #[arg(long, default_value = "project")]
        scope: String,
        /// Event type to hook into
        #[arg(long)]
        event: String,
        /// Matcher pattern for the hook (optional, defaults to empty string)
        #[arg(long, default_value = "")]
        matcher: String,
        /// Command to execute when hook is triggered
        #[arg(long)]
        command: String,
    },
    /// Delete hooks interactively
    Delete {
        /// Interactive mode to select and delete hooks
        #[arg(long, default_value = "true")]
        interactive: bool,
    },
}

#[derive(Subcommand)]
pub enum CommandsSubcommand {
    /// List all slash commands
    List {
        /// Scope: user or project (defaults to showing both)
        #[arg(long, value_enum)]
        scope: Option<Scope>,
    },
    /// Import slash command from GitHub markdown file
    Import {
        /// GitHub URL to the markdown file (e.g., https://github.com/owner/repo/blob/main/path/to/file.md)
        url: String,
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Remove all slash commands (with confirmation)
    Clean {
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Generate a command template
    #[command(alias = "gen")]
    Generate {
        /// The filename for the template (optional, defaults to command-template.md)
        filename: Option<String>,
    },
    /// Delete commands interactively
    Delete {
        /// Interactive mode to select and delete commands
        #[arg(short, long, default_value = "true")]
        interactive: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Scope {
    User,
    Project,
    #[value(name = "project.local")]
    ProjectLocal,
}

#[derive(Subcommand)]
pub enum SettingsSubcommand {
    /// Apply settings from a file or URL to Claude Code settings
    Apply {
        /// Path to local settings file or GitHub URL
        source: String,
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
        /// Overwrite existing settings (abort by default when duplicates exist)
        #[arg(long, default_value = "false")]
        overwrite: bool,
    },
}

#[derive(Subcommand)]
pub enum AgentsSubcommand {
    /// List all agents
    List {
        /// Scope: user or project (defaults to showing both)
        #[arg(long, value_enum)]
        scope: Option<Scope>,
    },
    /// Import agent from file or URL
    Import {
        /// Path to agent file or GitHub URL
        source: String,
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Delete agents interactively
    Delete {
        /// Interactive mode to select and delete agents
        #[arg(short, long, default_value = "true")]
        interactive: bool,
    },
    /// Remove all agents (with confirmation)
    Clean {
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Generate an agent template
    #[command(alias = "gen")]
    Generate {
        /// The filename for the template (optional, defaults to agent-template.md)
        filename: Option<String>,
    },
}
