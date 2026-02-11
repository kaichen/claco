use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use claco::claude::{save_settings, Settings};

/// Handle the repo command to initialize Claude Code project structure
pub fn handle_repo() -> Result<()> {
    let claude_dir = PathBuf::from(".claude");

    // Check if .claude directory already exists
    if claude_dir.exists() {
        println!("Claude Code project structure already exists at ./.claude");
        println!("To reinitialize, please remove the existing .claude directory first.");
        return Ok(());
    }

    // Create .claude directory
    fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;
    println!("Created .claude/");

    // Create agents subdirectory
    let agents_dir = claude_dir.join("agents");
    fs::create_dir_all(&agents_dir).context("Failed to create .claude/agents directory")?;
    println!("Created .claude/agents/");

    // Create commands subdirectory
    let commands_dir = claude_dir.join("commands");
    fs::create_dir_all(&commands_dir).context("Failed to create .claude/commands directory")?;
    println!("Created .claude/commands/");

    // Create settings.json with default configuration
    let settings_path = claude_dir.join("settings.json");
    let settings = Settings::default();
    save_settings(&settings_path, &settings).context("Failed to create .claude/settings.json")?;
    println!("Created .claude/settings.json");

    println!("\nClaude Code project structure initialized successfully!");
    println!("\nYou can now:");
    println!("  - Add custom agents to .claude/agents/");
    println!("  - Add custom commands to .claude/commands/");
    println!("  - Configure settings in .claude/settings.json");

    Ok(())
}
