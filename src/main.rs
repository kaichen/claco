use anyhow::Result;
use claco::{Cli, Commands};
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod commands;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Agents(cmd) => commands::handle_agents(cmd).await?,
        Commands::Commands(cmd) => commands::handle_commands(cmd).await?,
        Commands::Hooks { action } => commands::handle_hooks(action)?,
        Commands::History { session } => commands::handle_history(session)?,
        Commands::Session { session_id } => commands::handle_session(session_id)?,
        Commands::Projects => commands::handle_projects()?,
    }

    Ok(())
}
