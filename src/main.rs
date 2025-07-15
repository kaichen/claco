use anyhow::Result;
use chrono::{DateTime, Local};
use claco::{
    claude_home, desanitize_project_path, ide_dir, Cli, Commands, CommandsSubcommand, LockFile,
    Scope, SessionEntry,
};
use clap::Parser;
use regex::Regex;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn format_timestamp_local(timestamp_str: &str) -> String {
    // Try to parse the timestamp as UTC and convert to local timezone
    match DateTime::parse_from_rfc3339(timestamp_str) {
        Ok(dt) => {
            let local_dt: DateTime<Local> = dt.with_timezone(&Local);
            local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        Err(_) => timestamp_str.to_string(), // If parsing fails, return original
    }
}

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
        Commands::History { session } => handle_history(session)?,
        Commands::Session { session_id } => handle_session(session_id)?,
        Commands::Projects => handle_projects()?,
        Commands::Live => handle_live()?,
        Commands::Commands(cmd) => handle_commands(cmd).await?,
    }

    Ok(())
}

fn handle_history(session_id: Option<String>) -> Result<()> {
    // Get current working directory
    let cwd = std::env::current_dir()?;
    let cwd_str = cwd.to_string_lossy().to_string();

    let projects_dir = claude_home().join("projects");

    if !projects_dir.exists() {
        println!("No Claude projects directory found");
        return Ok(());
    }

    // Find the project directory that matches the current working directory
    let mut matched_project_path = None;

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Try to read the actual cwd from any JSONL file in this project
        for session_entry in fs::read_dir(&path)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();

            if session_path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                if let Ok(file) = fs::File::open(&session_path) {
                    let reader = BufReader::new(file);
                    if let Some(Ok(first_line)) = reader.lines().next() {
                        if let Ok(entry) = serde_json::from_str::<SessionEntry>(&first_line) {
                            if entry.cwd == cwd_str {
                                matched_project_path = Some(path.clone());
                                break;
                            }
                        }
                    }
                }
            }
        }

        if matched_project_path.is_some() {
            break;
        }
    }

    let project_path = match matched_project_path {
        Some(path) => path,
        None => {
            println!("No Claude project found for current directory: {}", cwd_str);
            return Ok(());
        }
    };

    // Read all session files or just the specified one
    let entries = fs::read_dir(&project_path)?;

    // Compile regex once for performance
    let command_regex = Regex::new(r"<command-name>(/[^<]+)</command-name>").unwrap();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            let file_name = path.file_stem().unwrap().to_string_lossy();

            // If session_id is specified, only process that session
            if let Some(ref sid) = session_id {
                if file_name != *sid {
                    continue;
                }
            }

            // Read and parse JSONL file
            let file = fs::File::open(&path)?;
            let reader = BufReader::new(file);

            let mut skip_next = false;

            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }

                // Skip this line if previous was a slash command
                if skip_next {
                    skip_next = false;
                    continue;
                }

                // Only process lines that contain "isSidechain":false, "type":"user" and "role":"user"
                if !(line.contains(r#""type":"user""#)
                    && line.contains(r#""role":"user""#)
                    && line.contains(r#""isSidechain":false"#))
                {
                    continue;
                }

                // Hard-coded caveat message to skip
                if line.contains(r#"Caveat: The messages below were generated by the user while running local commands."#) {
                    continue;
                }

                // Now try to parse the user message
                if let Ok(entry) = serde_json::from_str::<SessionEntry>(&line) {
                    // Check if the content contains a slash command
                    if let Some(captures) = command_regex.captures(&entry.message.content) {
                        // Print only the slash command
                        if let Some(command) = captures.get(1) {
                            println!(
                                "{}: {}",
                                format_timestamp_local(&entry.timestamp),
                                command.as_str()
                            );
                            // Skip the next line after a slash command
                            skip_next = true;
                        }
                    } else {
                        // No command-name tag found, print the full content
                        println!(
                            "{}: {}",
                            format_timestamp_local(&entry.timestamp),
                            entry.message.content
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_session(session_id: Option<String>) -> Result<()> {
    let projects_dir = claude_home().join("projects");

    if !projects_dir.exists() {
        println!("No Claude projects directory found");
        return Ok(());
    }

    // If no session_id provided, find the most recent session
    let target_session_id = if let Some(id) = session_id {
        id
    } else {
        // Find the most recent JSONL file across all projects
        let mut most_recent_session = None;
        let mut most_recent_time = None;

        for project_entry in fs::read_dir(&projects_dir)? {
            let project_entry = project_entry?;
            let project_path = project_entry.path();

            if !project_path.is_dir() {
                continue;
            }

            for session_entry in fs::read_dir(&project_path)? {
                let session_entry = session_entry?;
                let session_path = session_entry.path();

                if session_path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                    if let Ok(metadata) = session_entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if most_recent_time.is_none() || modified > most_recent_time.unwrap() {
                                most_recent_time = Some(modified);
                                most_recent_session = session_path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        match most_recent_session {
            Some(id) => {
                println!("Using most recent session: {}", id);
                id
            }
            None => {
                println!("No sessions found");
                return Ok(());
            }
        }
    };

    // Search all project directories for the session
    for project_entry in fs::read_dir(&projects_dir)? {
        let project_entry = project_entry?;
        let project_path = project_entry.path();

        if !project_path.is_dir() {
            continue;
        }

        let session_file = project_path.join(format!("{}.jsonl", target_session_id));

        if session_file.exists() {
            // Found the session
            println!("Session ID: {}", target_session_id);

            // Read first user message and get timestamp
            let file = fs::File::open(&session_file)?;
            let reader = BufReader::new(file);

            let mut first_user_message = None;
            let mut first_timestamp = None;
            let mut project_cwd = None;

            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }

                let entry: SessionEntry = serde_json::from_str(&line)?;

                if project_cwd.is_none() {
                    project_cwd = Some(entry.cwd.clone());
                }

                if first_timestamp.is_none() {
                    first_timestamp = Some(entry.timestamp.clone());
                }

                if entry.message_type == "user"
                    && entry.user_type == "external"
                    && first_user_message.is_none()
                {
                    first_user_message = Some(entry.message.content.clone());
                }
            }

            if let Some(cwd) = project_cwd {
                println!("Project: {}", cwd);
            }

            if let Some(timestamp) = first_timestamp {
                println!("Started: {}", format_timestamp_local(&timestamp));
            }

            if let Some(message) = first_user_message {
                println!("First user message: {}", message);
            }

            return Ok(());
        }
    }

    println!("Session not found: {}", target_session_id);
    Ok(())
}

fn handle_projects() -> Result<()> {
    let projects_dir = claude_home().join("projects");

    if !projects_dir.exists() {
        println!("No Claude projects directory found");
        return Ok(());
    }

    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Collect sessions and try to get the actual cwd from any JSONL file
        let mut sessions = Vec::new();
        let mut actual_cwd = None;

        for session_entry in fs::read_dir(&path)? {
            let session_entry = session_entry?;
            let session_path = session_entry.path();

            if session_path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                if let Some(session_id) = session_path.file_stem() {
                    sessions.push(session_id.to_string_lossy().to_string());
                }

                // Try to read the actual cwd from the first line of this JSONL file
                if actual_cwd.is_none() {
                    if let Ok(file) = fs::File::open(&session_path) {
                        let reader = BufReader::new(file);
                        if let Some(Ok(first_line)) = reader.lines().next() {
                            if let Ok(entry) = serde_json::from_str::<SessionEntry>(&first_line) {
                                actual_cwd = Some(entry.cwd);
                            }
                        }
                    }
                }
            }
        }

        // Use actual cwd if found, otherwise fall back to desanitized path
        let project_path = if let Some(cwd) = actual_cwd {
            cwd
        } else {
            let project_name = path.file_name().unwrap().to_string_lossy();
            desanitize_project_path(&project_name)
        };

        println!("Project: {}", project_path);
        println!("  Sessions: {:?}", sessions);
        println!();
    }

    Ok(())
}

fn handle_live() -> Result<()> {
    let ide_path = ide_dir();

    if !ide_path.exists() {
        println!("No active Claude sessions found");
        return Ok(());
    }

    for entry in fs::read_dir(&ide_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("lock") {
            // Read and parse lock file
            let content = fs::read_to_string(&path)?;
            let lock_info: LockFile = serde_json::from_str(&content)?;

            println!("Active session:");
            println!("  PID: {}", lock_info.pid);
            println!("  IDE: {}", lock_info.ide_name);
            println!("  Workspaces: {:?}", lock_info.workspace_folders);
            println!();
        }
    }

    Ok(())
}

async fn handle_commands(cmd: CommandsSubcommand) -> Result<()> {
    match cmd {
        CommandsSubcommand::List { scope } => handle_commands_list(scope)?,
        CommandsSubcommand::Import { url, scope } => handle_commands_import(url, scope).await?,
        CommandsSubcommand::Clean { scope } => handle_commands_clean(scope)?,
        CommandsSubcommand::Generate { prompt } => handle_commands_generate(prompt)?,
    }
    Ok(())
}

fn get_commands_dir(scope: &Scope) -> Result<std::path::PathBuf> {
    match scope {
        Scope::User => Ok(claude_home().join("commands")),
        Scope::Project => {
            let cwd = std::env::current_dir()?;
            Ok(cwd.join(".claude").join("commands"))
        }
    }
}

fn handle_commands_list(scope: Option<Scope>) -> Result<()> {
    match scope {
        Some(specific_scope) => {
            // Show commands for a specific scope
            let commands_dir = get_commands_dir(&specific_scope)?;

            if !commands_dir.exists() {
                println!("No commands directory found at: {}", commands_dir.display());
                return Ok(());
            }

            let scope_label = match specific_scope {
                Scope::User => "user",
                Scope::Project => "project",
            };

            println!(
                "Slash commands ({}): {}",
                scope_label,
                commands_dir.display()
            );
            println!();

            list_commands_recursive(&commands_dir, "", &specific_scope)?;
        }
        None => {
            // Show commands from both user and project scopes
            println!("Slash commands:");
            println!();

            // List user commands
            let user_scope = Scope::User;
            let user_commands_dir = get_commands_dir(&user_scope)?;
            if user_commands_dir.exists() {
                println!("User commands: {}", user_commands_dir.display());
                list_commands_recursive(&user_commands_dir, "", &user_scope)?;
                println!();
            }

            // List project commands
            let project_scope = Scope::Project;
            let project_commands_dir = get_commands_dir(&project_scope)?;
            if project_commands_dir.exists() {
                println!("Project commands: {}", project_commands_dir.display());
                list_commands_recursive(&project_commands_dir, "", &project_scope)?;
            } else {
                println!(
                    "No project commands found at: {}",
                    project_commands_dir.display()
                );
            }
        }
    }
    Ok(())
}

fn list_commands_recursive(dir: &std::path::Path, namespace: &str, scope: &Scope) -> Result<()> {
    let entries = fs::read_dir(dir)?;
    let mut commands = Vec::new();
    let mut subdirs = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            subdirs.push(entry.file_name().to_string_lossy().to_string());
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            if let Some(name) = path.file_stem() {
                commands.push(name.to_string_lossy().to_string());
            }
        }
    }

    // Sort both lists
    commands.sort();
    subdirs.sort();

    // Print commands in current directory
    for command in commands {
        let full_name = if namespace.is_empty() {
            format!("/{}", command)
        } else {
            format!("/{}:{}", namespace, command)
        };

        println!("  {}", full_name);
    }

    // Recursively process subdirectories
    for subdir in subdirs {
        let subdir_path = dir.join(&subdir);
        let new_namespace = if namespace.is_empty() {
            subdir.clone()
        } else {
            format!("{}:{}", namespace, subdir)
        };

        list_commands_recursive(&subdir_path, &new_namespace, scope)?;
    }

    Ok(())
}

async fn handle_commands_import(url: String, scope: Scope) -> Result<()> {
    use std::process::Command;

    // Check if gh is installed
    let gh_check = Command::new("gh").arg("--version").output();

    if gh_check.is_err() {
        anyhow::bail!(
            "GitHub CLI (gh) is not installed. Please install it from https://cli.github.com/"
        );
    }

    // Parse GitHub URL
    let parsed_url = url::Url::parse(&url)?;

    // Check if it's a GitHub URL
    if parsed_url.host_str() != Some("github.com") {
        anyhow::bail!("Only GitHub URLs are supported. Example: https://github.com/owner/repo/blob/main/path/to/file.md");
    }

    // Extract owner, repo, and path from GitHub URL
    // Format: https://github.com/owner/repo/blob/branch/path/to/file.md
    let path_segments: Vec<&str> = parsed_url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL"))?
        .collect();

    if path_segments.len() < 5 || path_segments[2] != "blob" {
        anyhow::bail!("Invalid GitHub URL format. Expected: https://github.com/owner/repo/blob/branch/path/to/file.md");
    }

    let owner = path_segments[0];
    let repo = path_segments[1];
    let branch = path_segments[3];
    let file_path_parts = &path_segments[4..];
    let file_path = file_path_parts.join("/");

    // Extract filename
    let filename = file_path_parts.last().unwrap_or(&"command.md");

    // Ensure it's a markdown file
    let filename = if filename.ends_with(".md") {
        filename.to_string()
    } else {
        anyhow::bail!("Only markdown files (.md) are supported for slash commands");
    };

    // Create commands directory if it doesn't exist
    let commands_dir = get_commands_dir(&scope)?;
    fs::create_dir_all(&commands_dir)?;

    let output_path = commands_dir.join(&filename);

    println!("Downloading command from GitHub...");

    // Use gh to download the file
    let output = Command::new("gh")
        .args(&[
            "api",
            &format!(
                "/repos/{}/{}/contents/{}?ref={}",
                owner, repo, file_path, branch
            ),
            "--jq",
            ".content",
            "-H",
            "Accept: application/vnd.github.v3+json",
        ])
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to download file from GitHub: {}", error);
    }

    // The content is base64 encoded, decode it
    let base64_content = String::from_utf8_lossy(&output.stdout);
    let base64_content = base64_content
        .trim()
        .replace("\\n", "")
        .replace("\n", "")
        .replace(" ", "");

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&base64_content)
        .map_err(|e| anyhow::anyhow!("Failed to decode base64 content: {}", e))?;

    let content = String::from_utf8(decoded)
        .map_err(|e| anyhow::anyhow!("File content is not valid UTF-8: {}", e))?;

    // Write the content to the file
    fs::write(&output_path, content)?;

    let scope_label = match scope {
        Scope::User => "user",
        Scope::Project => "project",
    };

    println!(
        "✓ Imported command '{}' from GitHub to {} scope: {}",
        filename.trim_end_matches(".md"),
        scope_label,
        output_path.display()
    );
    Ok(())
}

fn handle_commands_clean(scope: Scope) -> Result<()> {
    let commands_dir = get_commands_dir(&scope)?;

    if !commands_dir.exists() {
        println!("No commands directory found at: {}", commands_dir.display());
        return Ok(());
    }

    let scope_label = match scope {
        Scope::User => "user",
        Scope::Project => "project",
    };

    // Count existing commands
    let command_count = count_commands_recursive(&commands_dir)?;

    if command_count == 0 {
        println!("No commands found in {} scope", scope_label);
        return Ok(());
    }

    println!(
        "Found {} command(s) in {} scope at: {}",
        command_count,
        scope_label,
        commands_dir.display()
    );
    print!("Are you sure you want to remove all commands? (y/N): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes" {
        fs::remove_dir_all(&commands_dir)?;
        println!("Removed all commands from {} scope", scope_label);
    } else {
        println!("Operation cancelled");
    }

    Ok(())
}

fn count_commands_recursive(dir: &std::path::Path) -> Result<usize> {
    let mut count = 0;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            count += count_commands_recursive(&path)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            count += 1;
        }
    }

    Ok(count)
}

fn handle_commands_generate(prompt: String) -> Result<()> {
    use std::process::{Command, Stdio};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;

    println!("Launching Claude to generate command...");

    // Compact system prompt for slash command generation
    let system_prompt = r#"You are a slash command generator for Claude Code. Generate markdown files for custom slash commands following this format:

STRUCTURE:Organize commands in subdirectories. 
- Folder: command comes from the project directory (.claude/commands).
- Filename: command-name.md (becomes /command-name)
- Optional YAML frontmatter with:
  - description: Brief command description
  - allowed-tools: List of tools like Bash, Read, Edit, etc.
- Main content: Clear prompt instructions

FEATURES:
- Use $ARGUMENTS for dynamic values
- Use !`bash command` to execute commands and include output
- Use @filepath to reference file contents
- Commands can be namespaced in subdirectories (e.g., frontend/component.md becomes /frontend:component)

EXAMPLE:
```markdown
---
description: Review code for issues
allowed-tools: Read, Grep
---

Review the following code for potential issues:
@$ARGUMENTS

Focus on performance, security, and best practices.

Use Write tool to write down the result to markdown file.
```

Generate concise, practical commands that follow these conventions."#;

    let claude_prompt = format!("Generate a slash command markdown in this project for: {}\n\nProvide the complete markdown content including filename suggestion.", prompt);

    // Set up spinner
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    // Spawn spinner thread
    let spinner_handle = thread::spawn(move || {
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let mut i = 0;

        while running_clone.load(Ordering::Relaxed) {
            print!(
                "\r{} Generating command...",
                spinner_chars[i % spinner_chars.len()]
            );
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(100));
            i += 1;
        }

        // Clear the spinner line
        print!("\r                                        \r");
        io::stdout().flush().unwrap();
    });

    // Run the command
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg("--allowedTools")
        .arg("Edit,Read,Write")
        .arg("--append-system-prompt")
        .arg(system_prompt)
        .arg(&claude_prompt)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status()?;

    // Stop the spinner
    running.store(false, Ordering::Relaxed);
    spinner_handle.join().unwrap();

    if !status.success() {
        println!("Failed to launch Claude CLI. Make sure 'claude' is installed and in your PATH.");
    } else {
        println!("✓ Command generation completed");
    }

    Ok(())
}
