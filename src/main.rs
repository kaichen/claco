use anyhow::{Context, Result};
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

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set global tracing subscriber")?;

    match cli.command {
        Commands::Agents(cmd) => commands::handle_agents(cmd).await.context("Failed to handle agents command")?,
        Commands::Commands(cmd) => commands::handle_commands(cmd).await.context("Failed to handle commands command")?,
        Commands::Hooks { action } => commands::handle_hooks(action).context("Failed to handle hooks command")?,
        Commands::History { session } => commands::handle_history(session).context("Failed to handle history command")?,
        Commands::Session { session_id } => commands::handle_session(session_id).context("Failed to handle session command")?,
        Commands::Projects => commands::handle_projects().context("Failed to handle projects command")?,
        Commands::Settings(cmd) => commands::handle_settings(cmd).await.context("Failed to handle settings command")?,
    }

    Ok(())
}
