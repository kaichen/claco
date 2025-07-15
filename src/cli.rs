use clap::{Parser, Subcommand};

/// Claude Code CLI Inspector
#[derive(Parser)]
#[command(name = "claco")]
#[command(author, version, about = "Claude Code CLI Inspector", long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// List all active Claude sessions
    Live,
    /// Manage slash commands
    #[command(subcommand)]
    Commands(CommandsSubcommand),
}

#[derive(Subcommand)]
pub enum CommandsSubcommand {
    /// List all slash commands
    List {
        /// Scope: user or project (defaults to project)
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Import slash command from markdown file from GitHub
    Import {
        /// URL to the markdown file
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
    /// Generate command from prompt via Claude Code itself
    Generate {
        /// The prompt to generate a command from
        prompt: String,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Scope {
    User,
    Project,
}
