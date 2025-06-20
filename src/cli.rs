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
}
