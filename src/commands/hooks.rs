use anyhow::Result;
use claco::{
    load_settings, project_settings_path, save_settings, user_settings_path, Hook, HookMatcher,
    HooksAction,
};
use std::io::{self, Write};

/// Handle hook-related actions
///
/// This function processes all hook management operations including:
/// - Listing hooks from user/project scopes
/// - Adding new hooks with event patterns and commands
/// - Deleting hooks interactively
pub fn handle_hooks(action: HooksAction) -> Result<()> {
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
                "user" => user_settings_path()?,
                "project" => project_settings_path(),
                _ => {
                    eprintln!("error: invalid scope '{specific_scope}' - use 'user' or 'project'");
                    return Ok(());
                }
            };

            let settings = load_settings(&settings_path)?;

            if let Some(hooks) = &settings.hooks {
                println!("Hooks in {specific_scope} scope:");
                println!("Settings file: {}", settings_path.display());
                println!();

                if hooks.is_empty() {
                    println!("No hooks found");
                    return Ok(());
                }

                for (event, matchers) in hooks {
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
            let user_settings_path = user_settings_path()?;
            let user_settings = load_settings(&user_settings_path)?;

            if let Some(hooks) = &user_settings.hooks {
                if !hooks.is_empty() {
                    println!("User hooks: {}", user_settings_path.display());
                    for (event, matchers) in hooks {
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
                if !hooks.is_empty() {
                    println!("Project hooks: {}", project_settings_path.display());
                    for (event, matchers) in hooks {
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
        "user" => user_settings_path()?,
        "project" => project_settings_path(),
        _ => {
            eprintln!("error: invalid scope '{scope}' - use 'user' or 'project'");
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
        eprintln!("error: invalid event '{event}' - valid events are: {valid_events:?}");
        return Ok(());
    }

    let mut settings = load_settings(&settings_path)?;

    // Initialize hooks if not present
    if settings.hooks.is_none() {
        settings.hooks = Some(Default::default());
    }

    let hooks = settings.hooks.as_mut().unwrap();

    // Get or create the event entry
    let event_matchers = hooks.entry(event.clone()).or_insert_with(Vec::new);

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
        eprintln!("error: non-interactive mode is not supported yet");
        return Ok(());
    }

    // Load hooks from both scopes
    let user_settings_path = user_settings_path()?;
    let project_settings_path = project_settings_path();

    let user_settings = load_settings(&user_settings_path)?;
    let project_settings = load_settings(&project_settings_path)?;

    // Collect all hooks with their metadata
    let mut hooks_list = Vec::new();

    // Add user hooks
    if let Some(hooks) = &user_settings.hooks {
        for (event, matchers) in hooks {
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
        for (event, matchers) in hooks {
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
                if let Some(matchers) = hooks.get_mut(event) {
                    if let Some(matcher) = matchers.get_mut(*matcher_idx) {
                        if *hook_idx < matcher.hooks.len() {
                            matcher.hooks.remove(*hook_idx);
                            if matcher.hooks.is_empty() {
                                matchers.remove(*matcher_idx);
                            }
                        }
                    }
                    if matchers.is_empty() {
                        hooks.remove(event);
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
                if let Some(matchers) = hooks.get_mut(event) {
                    if let Some(matcher) = matchers.get_mut(*matcher_idx) {
                        if *hook_idx < matcher.hooks.len() {
                            matcher.hooks.remove(*hook_idx);
                            if matcher.hooks.is_empty() {
                                matchers.remove(*matcher_idx);
                            }
                        }
                    }
                    if matchers.is_empty() {
                        hooks.remove(event);
                    }
                }
            }
        }
        save_settings(&project_settings_path, &project_settings)?;
    }

    println!("Deleted {} hooks", indices_to_delete.len());

    Ok(())
}
