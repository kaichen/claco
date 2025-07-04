use anyhow::Result;
use claco::{claude_home, desanitize_project_path, ide_dir, Cli, Commands, LockFile, SessionEntry};
use clap::Parser;
use regex::Regex;
use std::fs;
use std::io::{BufRead, BufReader};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use chrono::{DateTime, Local};

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
                            println!("{}: {}", format_timestamp_local(&entry.timestamp), command.as_str());
                            // Skip the next line after a slash command
                            skip_next = true;
                        }
                    } else {
                        // No command-name tag found, print the full content
                        println!("{}: {}", format_timestamp_local(&entry.timestamp), entry.message.content);
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
