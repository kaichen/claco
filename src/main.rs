use anyhow::Result;
use chrono::{DateTime, Local};
use claco::{
    claude_home, desanitize_project_path, ide_dir, load_settings, project_settings_path, 
    save_settings, user_settings_path, Cli, Commands, Hook, HookMatcher, HooksAction, 
    LockFile, SessionEntry, Settings,
};
use clap::Parser;
use regex::Regex;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::collections::HashMap;
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
        Commands::Hooks { action } => handle_hooks(action)?,
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

fn handle_hooks(action: HooksAction) -> Result<()> {
    match action {
        HooksAction::List { scope } => handle_hooks_list(scope),
        HooksAction::Add { scope, event, matcher, command } => {
            handle_hooks_add(scope, event, matcher, command)
        }
        HooksAction::Remove { interactive } => handle_hooks_remove(interactive),
    }
}

fn handle_hooks_list(scope: String) -> Result<()> {
    let settings_path = match scope.as_str() {
        "user" => user_settings_path(),
        "project" => project_settings_path(),
        _ => {
            eprintln!("Error: Invalid scope '{}'. Use 'user' or 'project'", scope);
            return Ok(());
        }
    };

    let settings = load_settings(&settings_path)?;
    
    if let Some(hooks) = &settings.hooks {
        println!("Hooks in {} scope:", scope);
        println!("Settings file: {}", settings_path.display());
        println!();
        
        if hooks.events.is_empty() {
            println!("No hooks found");
            return Ok(());
        }
        
        for (event, matchers) in &hooks.events {
            println!("Event: {}", event);
            for matcher in matchers {
                for hook in &matcher.hooks {
                    println!("  {}:{}", matcher.matcher, hook.command);
                }
            }
            println!();
        }
    } else {
        println!("No hooks found in {} scope", scope);
    }
    
    Ok(())
}

fn handle_hooks_add(scope: String, event: String, matcher: String, command: String) -> Result<()> {
    let settings_path = match scope.as_str() {
        "user" => user_settings_path(),
        "project" => project_settings_path(),
        _ => {
            eprintln!("Error: Invalid scope '{}'. Use 'user' or 'project'", scope);
            return Ok(());
        }
    };

    // Validate event type
    let valid_events = vec![
        "PreToolUse", "ToolPattern", "Notification", "Stop", "SubagentStop", "PreCompact"
    ];
    if !valid_events.contains(&event.as_str()) {
        eprintln!("Error: Invalid event '{}'. Valid events are: {:?}", event, valid_events);
        return Ok(());
    }

    let mut settings = load_settings(&settings_path)?;
    
    // Initialize hooks if not present
    if settings.hooks.is_none() {
        settings.hooks = Some(Default::default());
    }
    
    let hooks = settings.hooks.as_mut().unwrap();
    
    // Get or create the event entry
    let event_matchers = hooks.events.entry(event.clone()).or_insert_with(Vec::new);
    
    // Find existing matcher or create new one
    let matcher_entry = event_matchers.iter_mut().find(|m| m.matcher == matcher);
    
    if let Some(matcher_entry) = matcher_entry {
        // Add hook to existing matcher
        matcher_entry.hooks.push(Hook {
            hook_type: "command".to_string(),
            command: command.clone(),
        });
    } else {
        // Create new matcher with the hook
        event_matchers.push(HookMatcher {
            matcher: matcher.clone(),
            hooks: vec![Hook {
                hook_type: "command".to_string(),
                command: command.clone(),
            }],
        });
    }
    
    save_settings(&settings_path, &settings)?;
    
    println!("Added hook: {} -> {}:{}", event, matcher, command);
    println!("Settings file: {}", settings_path.display());
    
    Ok(())
}

fn handle_hooks_remove(interactive: bool) -> Result<()> {
    if !interactive {
        eprintln!("Error: Non-interactive mode is not supported yet");
        return Ok(());
    }
    
    // Load hooks from both scopes
    let user_settings_path = user_settings_path();
    let project_settings_path = project_settings_path();
    
    let user_settings = load_settings(&user_settings_path)?;
    let project_settings = load_settings(&project_settings_path)?;
    
    // Collect all hooks with their metadata
    let mut hooks_list = Vec::new();
    
    // Add user hooks
    if let Some(hooks) = &user_settings.hooks {
        for (event, matchers) in &hooks.events {
            for (matcher_idx, matcher) in matchers.iter().enumerate() {
                for (hook_idx, hook) in matcher.hooks.iter().enumerate() {
                    hooks_list.push((
                        format!("{}:{}", matcher.matcher, hook.command),
                        "user".to_string(),
                        event.clone(),
                        matcher_idx,
                        hook_idx,
                    ));
                }
            }
        }
    }
    
    // Add project hooks
    if let Some(hooks) = &project_settings.hooks {
        for (event, matchers) in &hooks.events {
            for (matcher_idx, matcher) in matchers.iter().enumerate() {
                for (hook_idx, hook) in matcher.hooks.iter().enumerate() {
                    hooks_list.push((
                        format!("{}:{}", matcher.matcher, hook.command),
                        "project".to_string(),
                        event.clone(),
                        matcher_idx,
                        hook_idx,
                    ));
                }
            }
        }
    }
    
    if hooks_list.is_empty() {
        println!("No hooks found");
        return Ok(());
    }
    
    // Display hooks for selection
    println!("Select hooks to remove:");
    for (i, (hook_display, scope, event, _, _)) in hooks_list.iter().enumerate() {
        println!("{}. [{}] {}: {}", i + 1, scope, event, hook_display);
    }
    
    println!("\nEnter hook numbers to remove (comma-separated, or 'all' for all hooks):");
    print!("> ");
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    
    if input.is_empty() {
        println!("No hooks selected");
        return Ok(());
    }
    
    let indices_to_remove: Vec<usize> = if input == "all" {
        (0..hooks_list.len()).collect()
    } else {
        input.split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i > 0 && i <= hooks_list.len())
            .map(|i| i - 1)
            .collect()
    };
    
    if indices_to_remove.is_empty() {
        println!("No valid hooks selected");
        return Ok(());
    }
    
    // Group removals by scope
    let mut user_removals = Vec::new();
    let mut project_removals = Vec::new();
    
    for &idx in &indices_to_remove {
        let (_, scope, event, matcher_idx, hook_idx) = &hooks_list[idx];
        match scope.as_str() {
            "user" => user_removals.push((event.clone(), *matcher_idx, *hook_idx)),
            "project" => project_removals.push((event.clone(), *matcher_idx, *hook_idx)),
            _ => {}
        }
    }
    
    // Remove from user settings
    if !user_removals.is_empty() {
        let mut user_settings = load_settings(&user_settings_path)?;
        if let Some(hooks) = &mut user_settings.hooks {
            for (event, matcher_idx, hook_idx) in user_removals.iter().rev() {
                if let Some(matchers) = hooks.events.get_mut(event) {
                    if let Some(matcher) = matchers.get_mut(*matcher_idx) {
                        if *hook_idx < matcher.hooks.len() {
                            matcher.hooks.remove(*hook_idx);
                            if matcher.hooks.is_empty() {
                                matchers.remove(*matcher_idx);
                            }
                        }
                    }
                    if matchers.is_empty() {
                        hooks.events.remove(event);
                    }
                }
            }
        }
        save_settings(&user_settings_path, &user_settings)?;
    }
    
    // Remove from project settings
    if !project_removals.is_empty() {
        let mut project_settings = load_settings(&project_settings_path)?;
        if let Some(hooks) = &mut project_settings.hooks {
            for (event, matcher_idx, hook_idx) in project_removals.iter().rev() {
                if let Some(matchers) = hooks.events.get_mut(event) {
                    if let Some(matcher) = matchers.get_mut(*matcher_idx) {
                        if *hook_idx < matcher.hooks.len() {
                            matcher.hooks.remove(*hook_idx);
                            if matcher.hooks.is_empty() {
                                matchers.remove(*matcher_idx);
                            }
                        }
                    }
                    if matchers.is_empty() {
                        hooks.events.remove(event);
                    }
                }
            }
        }
        save_settings(&project_settings_path, &project_settings)?;
    }
    
    println!("Removed {} hooks", indices_to_remove.len());
    
    Ok(())
}
