use anyhow::Result;
use chrono::{DateTime, Local};
use claco::{
    claude_home, desanitize_project_path, ide_dir, load_settings, project_settings_path,
    save_settings, user_settings_path, AgentsSubcommand, Cli, Commands, CommandsSubcommand, Hook, HookMatcher,
    HooksAction, LockFile, Scope, SessionEntry,
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
        Commands::Hooks { action } => handle_hooks(action)?,
        Commands::Commands(cmd) => handle_commands(cmd).await?,
        Commands::Agents(cmd) => handle_agents(cmd).await?,
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
            println!("No Claude project found for current directory: {cwd_str}");
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
                println!("Using most recent session: {id}");
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

        let session_file = project_path.join(format!("{target_session_id}.jsonl"));

        if session_file.exists() {
            // Found the session
            println!("Session ID: {target_session_id}");

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
                println!("Project: {cwd}");
            }

            if let Some(timestamp) = first_timestamp {
                println!("Started: {}", format_timestamp_local(&timestamp));
            }

            if let Some(message) = first_user_message {
                println!("First user message: {message}");
            }

            return Ok(());
        }
    }

    println!("Session not found: {target_session_id}");
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

        println!("Project: {project_path}");
        println!("  Sessions: {sessions:?}");
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
        HooksAction::Add {
            scope,
            event,
            matcher,
            command,
        } => handle_hooks_add(scope, event, matcher, command),
        HooksAction::Delete { interactive } => handle_hooks_delete(interactive),
    }
}

fn handle_hooks_list(scope: Option<String>) -> Result<()> {
    match scope {
        Some(specific_scope) => {
            // Show hooks for a specific scope
            let settings_path = match specific_scope.as_str() {
                "user" => user_settings_path(),
                "project" => project_settings_path(),
                _ => {
                    eprintln!("Error: Invalid scope '{specific_scope}'. Use 'user' or 'project'");
                    return Ok(());
                }
            };

            let settings = load_settings(&settings_path)?;

            if let Some(hooks) = &settings.hooks {
                println!("Hooks in {specific_scope} scope:");
                println!("Settings file: {}", settings_path.display());
                println!();

                if hooks.events.is_empty() {
                    println!("No hooks found");
                    return Ok(());
                }

                for (event, matchers) in &hooks.events {
                    println!("Event: {event}");
                    for matcher in matchers {
                        for hook in &matcher.hooks {
                            let mut parts = vec![];
                            if !matcher.matcher.is_empty() {
                                parts.push(format!("matcher={}", matcher.matcher));
                            }
                            if !hook.command.is_empty() {
                                parts.push(format!("command=\"{}\"", hook.command));
                            }
                            if !hook.hook_type.is_empty() && hook.hook_type != "command" {
                                parts.push(format!("type={}", hook.hook_type));
                            }
                            println!("  {}", parts.join(" "));
                        }
                    }
                    println!();
                }
            } else {
                println!("No hooks found in {specific_scope} scope");
            }
        }
        None => {
            // Show hooks from both user and project scopes
            // List user hooks
            let user_settings_path = user_settings_path();
            let user_settings = load_settings(&user_settings_path)?;

            if let Some(hooks) = &user_settings.hooks {
                if !hooks.events.is_empty() {
                    println!("User hooks: {}", user_settings_path.display());
                    for (event, matchers) in &hooks.events {
                        println!("  Event: {event}");
                        for matcher in matchers {
                            for hook in &matcher.hooks {
                                let mut parts = vec![];
                                if !matcher.matcher.is_empty() {
                                    parts.push(format!("matcher={}", matcher.matcher));
                                }
                                if !hook.command.is_empty() {
                                    parts.push(format!("command=\"{}\"", hook.command));
                                }
                                if !hook.hook_type.is_empty() && hook.hook_type != "command" {
                                    parts.push(format!("type={}", hook.hook_type));
                                }
                                println!("    {}", parts.join(" "));
                            }
                        }
                    }
                    println!();
                }
            }

            // List project hooks
            let project_settings_path = project_settings_path();
            let project_settings = load_settings(&project_settings_path)?;

            if let Some(hooks) = &project_settings.hooks {
                if !hooks.events.is_empty() {
                    println!("Project hooks: {}", project_settings_path.display());
                    for (event, matchers) in &hooks.events {
                        println!("  Event: {event}");
                        for matcher in matchers {
                            for hook in &matcher.hooks {
                                let mut parts = vec![];
                                if !matcher.matcher.is_empty() {
                                    parts.push(format!("matcher={}", matcher.matcher));
                                }
                                if !hook.command.is_empty() {
                                    parts.push(format!("command=\"{}\"", hook.command));
                                }
                                if !hook.hook_type.is_empty() && hook.hook_type != "command" {
                                    parts.push(format!("type={}", hook.hook_type));
                                }
                                println!("    {}", parts.join(" "));
                            }
                        }
                    }
                } else {
                    println!(
                        "No project hooks found at: {}",
                        project_settings_path.display()
                    );
                }
            } else {
                println!(
                    "No project hooks found at: {}",
                    project_settings_path.display()
                );
            }
        }
    }

    Ok(())
}

fn handle_hooks_add(scope: String, event: String, matcher: String, command: String) -> Result<()> {
    let settings_path = match scope.as_str() {
        "user" => user_settings_path(),
        "project" => project_settings_path(),
        _ => {
            eprintln!("Error: Invalid scope '{scope}'. Use 'user' or 'project'");
            return Ok(());
        }
    };

    // Validate event type
    let valid_events = vec![
        "PreToolUse",
        "ToolPattern",
        "Notification",
        "Stop",
        "SubagentStop",
        "PreCompact",
    ];
    if !valid_events.contains(&event.as_str()) {
        eprintln!("Error: Invalid event '{event}'. Valid events are: {valid_events:?}");
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

    let mut parts = vec![];
    if !matcher.is_empty() {
        parts.push(format!("matcher={matcher}"));
    }
    if !command.is_empty() {
        parts.push(format!("command=\"{command}\""));
    }
    println!("Added hook: {} -> {}", event, parts.join(" "));
    println!("Settings file: {}", settings_path.display());

    Ok(())
}

fn handle_hooks_delete(interactive: bool) -> Result<()> {
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
                    let mut parts = vec![];
                    if !matcher.matcher.is_empty() {
                        parts.push(format!("matcher={}", matcher.matcher));
                    }
                    if !hook.command.is_empty() {
                        parts.push(format!("command=\"{}\"", hook.command));
                    }
                    if !hook.hook_type.is_empty() && hook.hook_type != "command" {
                        parts.push(format!("type={}", hook.hook_type));
                    }
                    hooks_list.push((
                        parts.join(" "),
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
                    let mut parts = vec![];
                    if !matcher.matcher.is_empty() {
                        parts.push(format!("matcher={}", matcher.matcher));
                    }
                    if !hook.command.is_empty() {
                        parts.push(format!("command=\"{}\"", hook.command));
                    }
                    if !hook.hook_type.is_empty() && hook.hook_type != "command" {
                        parts.push(format!("type={}", hook.hook_type));
                    }
                    hooks_list.push((
                        parts.join(" "),
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
    println!("Select hooks to delete:");
    for (i, (hook_display, scope, event, _, _)) in hooks_list.iter().enumerate() {
        println!("{}. [{}] {}: {}", i + 1, scope, event, hook_display);
    }

    println!("\nEnter hook numbers to delete (comma-separated, or 'all' for all hooks):");
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        println!("No hooks selected");
        return Ok(());
    }

    let indices_to_delete: Vec<usize> = if input == "all" {
        (0..hooks_list.len()).collect()
    } else {
        input
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i > 0 && i <= hooks_list.len())
            .map(|i| i - 1)
            .collect()
    };

    if indices_to_delete.is_empty() {
        println!("No valid hooks selected");
        return Ok(());
    }

    // Group removals by scope
    let mut user_removals = Vec::new();
    let mut project_removals = Vec::new();

    for &idx in &indices_to_delete {
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

    println!("Deleted {} hooks", indices_to_delete.len());

    Ok(())
}

async fn handle_commands(cmd: CommandsSubcommand) -> Result<()> {
    match cmd {
        CommandsSubcommand::List { scope } => handle_commands_list(scope)?,
        CommandsSubcommand::Import { url, scope } => handle_commands_import(url, scope).await?,
        CommandsSubcommand::Clean { scope } => handle_commands_clean(scope)?,
        CommandsSubcommand::Generate { prompt } => handle_commands_generate(prompt)?,
        CommandsSubcommand::Delete { interactive } => handle_commands_delete(interactive)?,
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

fn list_commands_recursive(dir: &std::path::Path, namespace: &str, _scope: &Scope) -> Result<()> {
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
            format!("/{command}")
        } else {
            format!("/{namespace}:{command}")
        };

        println!("  {full_name}");
    }

    // Recursively process subdirectories
    for subdir in subdirs {
        let subdir_path = dir.join(&subdir);
        let new_namespace = if namespace.is_empty() {
            subdir.clone()
        } else {
            format!("{namespace}:{subdir}")
        };

        list_commands_recursive(&subdir_path, &new_namespace, _scope)?;
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
        .args([
            "api",
            &format!("/repos/{owner}/{repo}/contents/{file_path}?ref={branch}"),
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
        println!("No commands found in {scope_label} scope");
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
        println!("Removed all commands from {scope_label} scope");
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

fn handle_commands_delete(interactive: bool) -> Result<()> {
    if !interactive {
        eprintln!("Error: Non-interactive mode is not supported yet");
        return Ok(());
    }

    // Collect all commands with their metadata
    let mut commands_list = Vec::new();

    // Add user commands
    let user_scope = Scope::User;
    let user_commands_dir = get_commands_dir(&user_scope)?;
    if user_commands_dir.exists() {
        collect_commands_recursive(&user_commands_dir, "", &user_scope, &mut commands_list)?;
    }

    // Add project commands
    let project_scope = Scope::Project;
    let project_commands_dir = get_commands_dir(&project_scope)?;
    if project_commands_dir.exists() {
        collect_commands_recursive(
            &project_commands_dir,
            "",
            &project_scope,
            &mut commands_list,
        )?;
    }

    if commands_list.is_empty() {
        println!("No commands found");
        return Ok(());
    }

    // Display commands for selection
    println!("Select commands to delete:");
    for (i, (command_name, scope, _file_path)) in commands_list.iter().enumerate() {
        let scope_label = match scope {
            Scope::User => "user",
            Scope::Project => "project",
        };
        println!("{}. [{}] {}", i + 1, scope_label, command_name);
    }

    println!("\nEnter command numbers to delete (comma-separated, or 'all' for all commands):");
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        println!("No commands selected");
        return Ok(());
    }

    let indices_to_delete: Vec<usize> = if input == "all" {
        (0..commands_list.len()).collect()
    } else {
        input
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i > 0 && i <= commands_list.len())
            .map(|i| i - 1)
            .collect()
    };

    if indices_to_delete.is_empty() {
        println!("No valid commands selected");
        return Ok(());
    }

    // Delete the selected commands
    let mut deleted_count = 0;
    for &idx in &indices_to_delete {
        let (_, _, file_path) = &commands_list[idx];
        if fs::remove_file(file_path).is_ok() {
            deleted_count += 1;

            // Clean up empty directories
            if let Some(parent) = file_path.parent() {
                // Try to remove parent directory if it's empty
                let _ = fs::remove_dir(parent);
            }
        }
    }

    println!("Deleted {deleted_count} command(s)");

    Ok(())
}

fn collect_commands_recursive(
    dir: &std::path::Path,
    namespace: &str,
    scope: &Scope,
    commands_list: &mut Vec<(String, Scope, std::path::PathBuf)>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let subdir_name = entry.file_name().to_string_lossy().to_string();
            let new_namespace = if namespace.is_empty() {
                subdir_name
            } else {
                format!("{namespace}:{subdir_name}")
            };
            collect_commands_recursive(&path, &new_namespace, scope, commands_list)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            if let Some(name) = path.file_stem() {
                let command_name = if namespace.is_empty() {
                    format!("/{}", name.to_string_lossy())
                } else {
                    format!("/{}:{}", namespace, name.to_string_lossy())
                };
                commands_list.push((command_name, scope.clone(), path.clone()));
            }
        }
    }
    Ok(())
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

    let claude_prompt = format!("Generate a slash command markdown in this project for: {prompt}\n\nProvide the complete markdown content including filename suggestion.");

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

// Agent management functions
async fn handle_agents(cmd: AgentsSubcommand) -> Result<()> {
    match cmd {
        AgentsSubcommand::List { scope } => handle_agents_list(scope)?,
        AgentsSubcommand::Import { source, scope } => handle_agents_import(source, scope).await?,
        AgentsSubcommand::Delete { interactive } => handle_agents_delete(interactive)?,
        AgentsSubcommand::Clean { scope } => handle_agents_clean(scope)?,
        AgentsSubcommand::Generate { prompt } => handle_agents_generate(prompt)?,
    }
    Ok(())
}

fn get_agents_dir(scope: &Scope) -> Result<std::path::PathBuf> {
    match scope {
        Scope::User => Ok(claude_home().join("agents")),
        Scope::Project => {
            let cwd = std::env::current_dir()?;
            Ok(cwd.join(".claude").join("agents"))
        }
    }
}

fn handle_agents_list(scope: Option<Scope>) -> Result<()> {
    match scope {
        Some(specific_scope) => {
            // Show agents for a specific scope
            let agents_dir = get_agents_dir(&specific_scope)?;

            if !agents_dir.exists() {
                println!("No agents directory found at: {}", agents_dir.display());
                return Ok(());
            }

            let scope_label = match specific_scope {
                Scope::User => "user",
                Scope::Project => "project",
            };

            println!(
                "Custom agents ({}): {}",
                scope_label,
                agents_dir.display()
            );
            println!();

            list_agents_recursive(&agents_dir, "", &specific_scope)?;
        }
        None => {
            // Show agents from both user and project scopes
            // List user agents
            let user_scope = Scope::User;
            let user_agents_dir = get_agents_dir(&user_scope)?;

            if user_agents_dir.exists() {
                println!(
                    "Custom agents (user): {}",
                    user_agents_dir.display()
                );
                println!();
                list_agents_recursive(&user_agents_dir, "", &user_scope)?;
                println!();
            }

            // List project agents
            let project_scope = Scope::Project;
            let project_agents_dir = get_agents_dir(&project_scope)?;

            if project_agents_dir.exists() {
                println!(
                    "Custom agents (project): {}",
                    project_agents_dir.display()
                );
                println!();
                list_agents_recursive(&project_agents_dir, "", &project_scope)?;
            } else {
                if !user_agents_dir.exists() {
                    println!("No agents found in user or project directories.");
                }
            }
        }
    }

    Ok(())
}

fn list_agents_recursive(
    dir: &std::path::Path,
    namespace: &str,
    scope: &Scope,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();

    // Sort entries: directories first, then files
    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        if path.is_dir() {
            let new_namespace = if namespace.is_empty() {
                file_name_str.to_string()
            } else {
                format!("{}/{}", namespace, file_name_str)
            };
            list_agents_recursive(&path, &new_namespace, scope)?;
        } else if file_name_str.ends_with(".md") {
            let agent_name = file_name_str.strip_suffix(".md").unwrap();
            let full_agent_name = if namespace.is_empty() {
                agent_name.to_string()
            } else {
                format!("{}/{}", namespace, agent_name)
            };

            // Try to read and parse agent metadata
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(agent_info) = parse_agent_metadata(&content) {
                    let scope_label = match scope {
                        Scope::User => "(user)",
                        Scope::Project => "(project)",
                    };
                    println!(
                        "- {} {} - {} [{}]",
                        full_agent_name,
                        scope_label,
                        agent_info.agent_type,
                        agent_info.when_to_use
                    );
                } else {
                    let scope_label = match scope {
                        Scope::User => "(user)",
                        Scope::Project => "(project)",
                    };
                    println!("- {} {} - (no metadata)", full_agent_name, scope_label);
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct AgentInfo {
    agent_type: String,
    when_to_use: String,
    allowed_tools: Vec<String>,
}

fn parse_agent_metadata(content: &str) -> Option<AgentInfo> {
    // Check if content starts with YAML frontmatter
    if !content.starts_with("---\n") {
        return None;
    }

    // Find the end of frontmatter
    let parts: Vec<&str> = content.splitn(3, "---\n").collect();
    if parts.len() < 3 {
        return None;
    }

    let frontmatter = parts[1];
    
    // Parse YAML frontmatter manually (simple parser for our specific fields)
    let mut agent_type = String::new();
    let mut when_to_use = String::new();
    let mut allowed_tools = Vec::new();

    for line in frontmatter.lines() {
        if let Some(value) = line.strip_prefix("agent-type: ") {
            agent_type = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("when-to-use: ") {
            when_to_use = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("allowed-tools: ") {
            // Parse list format: ["*"] or ["Read", "Write", "Edit"]
            if let Some(list_str) = value.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                allowed_tools = list_str
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect();
            }
        }
    }

    if agent_type.is_empty() {
        return None;
    }

    Some(AgentInfo {
        agent_type,
        when_to_use,
        allowed_tools,
    })
}

async fn handle_agents_import(source: String, scope: Scope) -> Result<()> {
    // Check if source is a URL or file path
    if source.starts_with("http://") || source.starts_with("https://") {
        // Import from URL (GitHub)
        handle_agents_import_from_url(source, scope).await
    } else {
        // Import from local file
        handle_agents_import_from_file(source, scope)
    }
}

async fn handle_agents_import_from_url(url: String, scope: Scope) -> Result<()> {
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
        anyhow::bail!("Only GitHub URLs are supported. Example: https://github.com/owner/repo/blob/main/path/to/agent.md");
    }

    // Extract owner, repo, and path from GitHub URL
    let path_segments: Vec<&str> = parsed_url.path_segments().unwrap().collect();
    
    if path_segments.len() < 5 || path_segments[2] != "blob" {
        anyhow::bail!("Invalid GitHub URL format. Expected: https://github.com/owner/repo/blob/branch/path/to/agent.md");
    }

    let owner = path_segments[0];
    let repo = path_segments[1];
    let branch = path_segments[3];
    let file_path = path_segments[4..].join("/");

    // Download the file using gh api
    println!("Downloading agent from GitHub...");
    let api_path = format!("repos/{}/{}/contents/{}?ref={}", owner, repo, file_path, branch);
    
    let output = Command::new("gh")
        .args(&["api", &api_path, "--jq", ".content"])
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to download agent: {}", error);
    }

    // Decode base64 content
    let base64_content = String::from_utf8(output.stdout)?;
    let base64_content = base64_content.trim();
    
    use base64::{engine::general_purpose, Engine as _};
    let content = general_purpose::STANDARD.decode(base64_content)?;
    let content_str = String::from_utf8(content)?;

    // Extract filename from URL
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("agent.md"));

    // Save the agent
    save_agent_content(&content_str, filename.to_string_lossy().as_ref(), scope)?;

    Ok(())
}

fn handle_agents_import_from_file(file_path: String, scope: Scope) -> Result<()> {
    let path = std::path::Path::new(&file_path);
    
    if !path.exists() {
        anyhow::bail!("File not found: {}", file_path);
    }

    if !path.is_file() {
        anyhow::bail!("Path is not a file: {}", file_path);
    }

    // Read the file content
    let content = fs::read_to_string(path)?;

    // Get the filename
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent.md");

    // Save the agent
    save_agent_content(&content, filename, scope)?;

    Ok(())
}

fn save_agent_content(content: &str, filename: &str, scope: Scope) -> Result<()> {
    // Validate the agent content
    if let Some(agent_info) = parse_agent_metadata(content) {
        println!("Importing agent: {}", agent_info.agent_type);
        println!("When to use: {}", agent_info.when_to_use);
        println!("Allowed tools: {:?}", agent_info.allowed_tools);
    } else {
        println!("Warning: Agent file does not contain valid metadata");
    }

    // Get the agents directory
    let agents_dir = get_agents_dir(&scope)?;

    // Create the directory if it doesn't exist
    fs::create_dir_all(&agents_dir)?;

    // Save the agent file
    let agent_path = agents_dir.join(filename);
    fs::write(&agent_path, content)?;

    println!("✓ Agent imported successfully to: {}", agent_path.display());

    Ok(())
}

fn handle_agents_delete(interactive: bool) -> Result<()> {
    if !interactive {
        eprintln!("Error: Non-interactive mode is not supported yet");
        return Ok(());
    }

    // Collect all agents with their metadata
    let mut agents_list = Vec::new();

    // Add user agents
    let user_scope = Scope::User;
    let user_agents_dir = get_agents_dir(&user_scope)?;
    if user_agents_dir.exists() {
        collect_agents_recursive(&user_agents_dir, "", &user_scope, &mut agents_list)?;
    }

    // Add project agents
    let project_scope = Scope::Project;
    let project_agents_dir = get_agents_dir(&project_scope)?;
    if project_agents_dir.exists() {
        collect_agents_recursive(
            &project_agents_dir,
            "",
            &project_scope,
            &mut agents_list,
        )?;
    }

    if agents_list.is_empty() {
        println!("No agents found");
        return Ok(());
    }

    // Display agents for selection
    println!("Select agents to delete:");
    for (i, (agent_name, scope, _file_path)) in agents_list.iter().enumerate() {
        let scope_label = match scope {
            Scope::User => "user",
            Scope::Project => "project",
        };
        println!("{}. [{}] {}", i + 1, scope_label, agent_name);
    }

    println!("\nEnter agent numbers to delete (comma-separated, or 'all' for all agents):");
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        println!("No agents selected");
        return Ok(());
    }

    let indices_to_delete: Vec<usize> = if input == "all" {
        (0..agents_list.len()).collect()
    } else {
        input
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .filter(|&i| i > 0 && i <= agents_list.len())
            .map(|i| i - 1)
            .collect()
    };

    if indices_to_delete.is_empty() {
        println!("No valid agents selected");
        return Ok(());
    }

    // Delete the selected agents
    let mut deleted_count = 0;
    for &idx in &indices_to_delete {
        let (_, _, file_path) = &agents_list[idx];
        if fs::remove_file(file_path).is_ok() {
            deleted_count += 1;

            // Clean up empty directories
            if let Some(parent) = file_path.parent() {
                // Try to remove parent directory if it's empty
                let _ = fs::remove_dir(parent);
            }
        }
    }

    println!("Deleted {deleted_count} agent(s)");

    Ok(())
}

fn collect_agents_recursive(
    dir: &std::path::Path,
    namespace: &str,
    scope: &Scope,
    agents_list: &mut Vec<(String, Scope, std::path::PathBuf)>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        if path.is_dir() {
            let new_namespace = if namespace.is_empty() {
                file_name_str.to_string()
            } else {
                format!("{}/{}", namespace, file_name_str)
            };
            collect_agents_recursive(&path, &new_namespace, scope, agents_list)?;
        } else if file_name_str.ends_with(".md") {
            let agent_name = file_name_str.strip_suffix(".md").unwrap();
            let full_agent_name = if namespace.is_empty() {
                agent_name.to_string()
            } else {
                format!("{}/{}", namespace, agent_name)
            };
            agents_list.push((full_agent_name, scope.clone(), path.clone()));
        }
    }
    Ok(())
}

fn handle_agents_clean(scope: Scope) -> Result<()> {
    let agents_dir = get_agents_dir(&scope)?;

    if !agents_dir.exists() {
        println!("No agents directory found at: {}", agents_dir.display());
        return Ok(());
    }

    // Count agents first
    let agent_count = count_files_in_dir(&agents_dir)?;

    if agent_count == 0 {
        println!("No agents found in {}", agents_dir.display());
        return Ok(());
    }

    let scope_label = match scope {
        Scope::User => "user",
        Scope::Project => "project",
    };

    println!(
        "This will delete {} agent(s) from {} scope.",
        agent_count, scope_label
    );
    println!("Directory: {}", agents_dir.display());
    print!("Are you sure? (y/N): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("Operation cancelled");
        return Ok(());
    }

    // Remove the entire agents directory
    fs::remove_dir_all(&agents_dir)?;
    println!("✓ Removed {} agent(s)", agent_count);

    Ok(())
}

fn count_files_in_dir(dir: &std::path::Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            count += count_files_in_dir(&path)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            count += 1;
        }
    }
    Ok(count)
}

fn handle_agents_generate(prompt: String) -> Result<()> {
    use std::process::{Command, Stdio};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;

    println!("Launching Claude to generate agent...");

    // System prompt for agent generation
    let system_prompt = r#"You are an agent generator for Claude Code. Generate markdown files for custom agents following this format:

STRUCTURE:
- Store in .claude/agents/ directory
- Filename: agent-name.md (descriptive name)
- Required YAML frontmatter with:
  - agent-type: descriptive-name-of-agent
  - when-to-use: Brief description of when to use this agent
  - allowed-tools: ["*"] or specific tools like ["Read", "Edit", "Bash"]

CONTENT:
- Clear instructions for the agent's specialized behavior
- Define the agent's expertise and approach
- Include specific guidelines and best practices
- Use second person ("You are...") for instructions

EXAMPLE:
```markdown
---
agent-type: security-analyst
when-to-use: Security vulnerability analysis, threat modeling, and security best practices recommendations
allowed-tools: ["*"]
---

You are a security specialist focused on identifying vulnerabilities and recommending secure coding practices.

When analyzing code:
- Check for common vulnerabilities (SQL injection, XSS, CSRF, etc.)
- Review authentication and authorization implementations
- Identify potential data exposure risks
- Suggest security improvements following OWASP guidelines

Always explain the severity and potential impact of identified issues.
```

Generate specialized, practical agents that provide clear value for specific tasks."#;

    let claude_prompt = format!("Generate a custom agent markdown for: {prompt}\n\nProvide the complete markdown content including filename suggestion.");

    // Set up spinner
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let spinner_handle = thread::spawn(move || {
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let mut i = 0;
        while running_clone.load(Ordering::Relaxed) {
            print!("\r{} ", spinner_chars[i % spinner_chars.len()]);
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(100));
            i += 1;
        }
        print!("\r  \r");
        io::stdout().flush().unwrap();
    });

    // Launch Claude with the prompt
    let mut cmd = Command::new("claude")
        .arg("--no-color")
        .arg("--system")
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
        println!("✓ Agent generation completed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_metadata_valid() {
        let content = r#"---
agent-type: security-analyst
when-to-use: Security vulnerability analysis, threat modeling, and security best practices recommendations
allowed-tools: ["*"]
---

You are a security specialist focused on identifying vulnerabilities and recommending secure coding practices.
"#;

        let agent_info = parse_agent_metadata(content).unwrap();
        assert_eq!(agent_info.agent_type, "security-analyst");
        assert_eq!(
            agent_info.when_to_use,
            "Security vulnerability analysis, threat modeling, and security best practices recommendations"
        );
        assert_eq!(agent_info.allowed_tools, vec!["*"]);
    }

    #[test]
    fn test_parse_agent_metadata_with_multiple_tools() {
        let content = r#"---
agent-type: code-reviewer
when-to-use: Code review and best practices
allowed-tools: ["Read", "Edit", "Bash"]
---

Review code for best practices.
"#;

        let agent_info = parse_agent_metadata(content).unwrap();
        assert_eq!(agent_info.agent_type, "code-reviewer");
        assert_eq!(agent_info.allowed_tools, vec!["Read", "Edit", "Bash"]);
    }

    #[test]
    fn test_parse_agent_metadata_invalid_no_frontmatter() {
        let content = "Just some content without frontmatter";
        assert!(parse_agent_metadata(content).is_none());
    }

    #[test]
    fn test_parse_agent_metadata_missing_agent_type() {
        let content = r#"---
when-to-use: Some description
allowed-tools: ["*"]
---

Content
"#;
        assert!(parse_agent_metadata(content).is_none());
    }

    #[test]
    fn test_get_agents_dir() {
        let user_dir = get_agents_dir(&Scope::User).unwrap();
        assert!(user_dir.to_string_lossy().contains("agents"));
        
        let project_dir = get_agents_dir(&Scope::Project).unwrap();
        assert!(project_dir.to_string_lossy().contains(".claude"));
        assert!(project_dir.to_string_lossy().contains("agents"));
    }
}
