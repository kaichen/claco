use anyhow::{Context, Result};
use claco::claude::{project_settings_path, save_settings, Settings};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Handle the init command
pub fn handle_init(force: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let claude_dir = current_dir.join(".claude");

    // Check if .claude directory already exists
    if claude_dir.exists() && !force {
        println!("Claude Code project already initialized in this directory.");
        println!("Use --force to reinitialize the project.");
        return Ok(());
    }

    // Create .claude directory structure
    println!(
        "Initializing Claude Code project in: {}",
        current_dir.display()
    );

    create_directory_structure(&claude_dir)?;
    create_default_settings()?;
    create_gitignore(&current_dir)?;

    println!("\n✓ Successfully initialized Claude Code project!");
    println!("\nProject structure created:");
    println!("  .claude/");
    println!("    ├── settings.json       (project settings)");
    println!("    ├── agents/             (custom agents)");
    println!("    └── commands/           (slash commands)");
    println!("\nNext steps:");
    println!("  • Add custom agents:   claco agents import <source>");
    println!("  • Add slash commands:  claco commands import <source>");
    println!("  • Configure hooks:     claco hooks add --event <event> --command <cmd>");
    println!("  • Start Claude Code with access to this project's configuration");

    Ok(())
}

/// Create the directory structure for a Claude Code project
fn create_directory_structure(claude_dir: &PathBuf) -> Result<()> {
    // Create main .claude directory
    fs::create_dir_all(claude_dir).context(format!(
        "Failed to create directory: {}",
        claude_dir.display()
    ))?;

    // Create subdirectories
    let agents_dir = claude_dir.join("agents");
    let commands_dir = claude_dir.join("commands");

    fs::create_dir_all(&agents_dir).context(format!(
        "Failed to create directory: {}",
        agents_dir.display()
    ))?;

    fs::create_dir_all(&commands_dir).context(format!(
        "Failed to create directory: {}",
        commands_dir.display()
    ))?;

    Ok(())
}

/// Create default settings.json file
fn create_default_settings() -> Result<()> {
    let settings_path = project_settings_path();

    // Only create if it doesn't exist
    if !settings_path.exists() {
        let default_settings = Settings::default();
        save_settings(&settings_path, &default_settings)
            .context("Failed to create default settings.json")?;
    }

    Ok(())
}

/// Create or update .gitignore to exclude settings.local.json
fn create_gitignore(current_dir: &Path) -> Result<()> {
    let gitignore_path = current_dir.join(".gitignore");
    let settings_local_entry = ".claude/settings.local.json\n";

    // Check if .gitignore exists
    if gitignore_path.exists() {
        // Read existing content
        let content = fs::read_to_string(&gitignore_path).context("Failed to read .gitignore")?;

        // Check if the entry already exists
        if !content.contains("settings.local.json") {
            // Append the entry
            let mut updated_content = content;
            if !updated_content.ends_with('\n') {
                updated_content.push('\n');
            }
            updated_content.push_str(settings_local_entry);

            fs::write(&gitignore_path, updated_content).context("Failed to update .gitignore")?;
        }
    } else {
        // Create new .gitignore
        fs::write(&gitignore_path, settings_local_entry).context("Failed to create .gitignore")?;
    }

    Ok(())
}
