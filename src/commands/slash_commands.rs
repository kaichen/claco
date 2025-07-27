use anyhow::Result;
use claco::{claude_home, generate_command, CommandsSubcommand, Scope};
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

pub async fn handle_commands(cmd: CommandsSubcommand) -> Result<()> {
    match cmd {
        CommandsSubcommand::List { scope } => handle_commands_list(scope)?,
        CommandsSubcommand::Import { url, scope } => handle_commands_import(url, scope).await?,
        CommandsSubcommand::Clean { scope } => handle_commands_clean(scope)?,
        CommandsSubcommand::Generate { prompt, template } => {
            handle_commands_generate(prompt, template)?
        }
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
        anyhow::bail!("Only GitHub URLs are supported. Example: https://github.com/owner/repo/blob/main/path/to/file.md or https://github.com/owner/repo/tree/main/path/to/folder");
    }

    // Extract owner, repo, and path from GitHub URL
    let path_segments: Vec<&str> = parsed_url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL: No path segments"))?
        .collect();

    if path_segments.len() < 4 {
        anyhow::bail!("Invalid GitHub URL format. Expected: https://github.com/owner/repo/blob/branch/path or https://github.com/owner/repo/tree/branch/path");
    }

    // Check if it's a tree (folder) or blob (file) URL
    let url_type = path_segments.get(2).copied();

    match url_type {
        Some("blob") => {
            if path_segments.len() < 5 {
                anyhow::bail!("Invalid file URL format. Expected: https://github.com/owner/repo/blob/branch/path/to/command.md");
            }
            // Import single file
            import_single_command_from_github(&path_segments, scope).await
        }
        Some("tree") => {
            // Import all .md files from folder
            import_commands_folder_from_github(&path_segments, scope).await
        }
        _ => {
            anyhow::bail!("Invalid GitHub URL format. URL must contain either 'blob' (for files) or 'tree' (for folders)");
        }
    }
}

async fn import_single_command_from_github(path_segments: &[&str], scope: Scope) -> Result<()> {
    let owner = path_segments[0];
    let repo = path_segments[1];
    let branch = path_segments[3];
    let file_path_parts = &path_segments[4..];
    let file_path = file_path_parts.join("/");

    // Validate components don't contain dangerous characters
    for component in [owner, repo, branch] {
        if component.contains([
            '$', '`', '\\', '"', '\'', '\n', '\r', ';', '|', '&', '<', '>', '(', ')',
        ]) {
            anyhow::bail!("Invalid characters in URL component: {}", component);
        }
    }

    // Additional validation for file path
    if file_path.contains("..") {
        anyhow::bail!("Invalid file path in URL: Path traversal detected");
    }

    // Extract filename
    let filename = file_path_parts.last().unwrap_or(&"command.md");

    // Ensure it's a markdown file
    if !filename.ends_with(".md") {
        anyhow::bail!("Only markdown files (.md) are supported for slash commands");
    }

    // Validate filename for security
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
        anyhow::bail!(
            "Invalid filename '{}': Path traversal not allowed",
            filename
        );
    }

    // Check for other dangerous characters
    if filename.contains('\0') {
        anyhow::bail!("Invalid filename '{}': Contains null byte", filename);
    }

    // Create commands directory if it doesn't exist
    let commands_dir = get_commands_dir(&scope)?;
    fs::create_dir_all(&commands_dir)?;

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
    // GitHub returns base64 with newlines, we need to remove all whitespace
    let base64_content: String = base64_content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    // Check size before decoding to prevent memory exhaustion
    const MAX_COMMAND_SIZE: usize = 10 * 1024 * 1024; // 10MB limit
    let estimated_size = (base64_content.len() * 3) / 4;
    if estimated_size > MAX_COMMAND_SIZE {
        anyhow::bail!(
            "Command file too large: estimated {} bytes, max {} bytes",
            estimated_size,
            MAX_COMMAND_SIZE
        );
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&base64_content)
        .map_err(|e| anyhow::anyhow!("Failed to decode base64 content: {}", e))?;

    // Verify actual size after decoding
    if decoded.len() > MAX_COMMAND_SIZE {
        anyhow::bail!(
            "Command file too large: {} bytes, max {} bytes",
            decoded.len(),
            MAX_COMMAND_SIZE
        );
    }

    let content = String::from_utf8(decoded)
        .map_err(|e| anyhow::anyhow!("File content is not valid UTF-8: {}", e))?;

    // Write the content to the file
    let output_path = commands_dir.join(filename);
    fs::write(&output_path, content)?;

    let scope_label = match scope {
        Scope::User => "user",
        Scope::Project => "project",
    };

    println!(
        "[OK] Imported command '{}' from GitHub to {} scope: {}",
        filename.trim_end_matches(".md"),
        scope_label,
        output_path.display()
    );
    Ok(())
}

async fn import_commands_folder_from_github(path_segments: &[&str], scope: Scope) -> Result<()> {
    let owner = path_segments[0];
    let repo = path_segments[1];
    let branch = path_segments[3];
    let folder_path = if path_segments.len() > 4 {
        path_segments[4..].join("/")
    } else {
        String::new()
    };

    // Validate components don't contain dangerous characters
    for component in [owner, repo, branch] {
        if component.contains([
            '$', '`', '\\', '"', '\'', '\n', '\r', ';', '|', '&', '<', '>', '(', ')',
        ]) {
            anyhow::bail!("Invalid characters in URL component: {}", component);
        }
    }

    // Additional validation for folder path
    if folder_path.contains("..") {
        anyhow::bail!("Invalid folder path in URL: Path traversal detected");
    }

    // List files in the folder using gh api
    println!("Listing commands in GitHub folder...");
    let api_path = format!("repos/{owner}/{repo}/contents/{folder_path}?ref={branch}");

    let output = Command::new("gh").args(["api", &api_path]).output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to list folder contents: {}", error);
    }

    // Parse JSON response
    let json_str = String::from_utf8(output.stdout)?;
    let files: serde_json::Value = serde_json::from_str(&json_str)?;

    // Filter for .md files
    let md_files: Vec<&serde_json::Value> = files
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON array response"))?
        .iter()
        .filter(|file| {
            file.get("type").and_then(|t| t.as_str()) == Some("file")
                && file
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.ends_with(".md"))
                    .unwrap_or(false)
        })
        .collect();

    if md_files.is_empty() {
        println!("No .md files found in the specified folder");
        return Ok(());
    }

    println!("Found {} command file(s) to import", md_files.len());

    let mut imported_count = 0;
    let mut failed_count = 0;

    // Import each .md file
    for file in md_files {
        let file_name = file
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing file name"))?;

        let file_path = if folder_path.is_empty() {
            file_name.to_string()
        } else {
            format!("{folder_path}/{file_name}")
        };

        println!("Importing {file_name}...");

        // Build the blob URL path segments
        let mut file_segments = vec![owner, repo, "blob", branch];
        file_segments.extend(file_path.split('/'));

        match import_single_command_from_github(&file_segments, scope.clone()).await {
            Ok(_) => imported_count += 1,
            Err(e) => {
                eprintln!("Failed to import {file_name}: {e}");
                failed_count += 1;
            }
        }
    }

    println!("\n[OK] Import complete: {imported_count} succeeded, {failed_count} failed");

    if failed_count > 0 {
        anyhow::bail!("Some imports failed");
    }

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

fn handle_commands_generate(prompt: String, template: bool) -> Result<()> {
    if template {
        // Generate template markdown
        let template_content = r#"---
description: Brief description of what this command does
tools: Read, Edit, Bash
---

# Command Name

Describe what this command does here.

## Instructions

$ARGUMENTS

## Example Usage

- Use $ARGUMENTS for command arguments
- Use @filepath to include file contents  
- Use !`command` to execute shell commands
"#;

        let filename = if prompt.is_empty() {
            "command-template.md".to_string()
        } else {
            // Sanitize the prompt to create a filename
            let sanitized = prompt
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            format!("{sanitized}.md")
        };

        // Get the project commands directory
        let commands_dir = get_commands_dir(&Scope::Project)?;
        fs::create_dir_all(&commands_dir)?;

        let output_path = commands_dir.join(&filename);

        // Check if file already exists
        if output_path.exists() {
            print!(
                "File {} already exists. Overwrite? (y/N): ",
                output_path.display()
            );
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase() != "y" {
                println!("Operation cancelled");
                return Ok(());
            }
        }

        // Write the template
        fs::write(&output_path, template_content)?;

        println!("[OK] Created command template: {}", output_path.display());
        println!("\nNext steps:");
        println!("  1. Edit the file to customize your command");
        println!("  2. Update the frontmatter properties");
        println!("  3. Replace placeholder content with actual instructions");
        println!("  4. Test it with: /{}", filename.trim_end_matches(".md"));

        return Ok(());
    }

    println!("Launching Claude to generate command...");

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

    // Use the new wrapper to generate command
    let result = generate_command(&prompt);

    // Stop the spinner
    running.store(false, Ordering::Relaxed);
    spinner_handle.join().unwrap();

    let (filename, content) = match result {
        Ok((f, c)) => (f, c),
        Err(e) => {
            println!("Failed to generate command: {e}");
            return Ok(());
        }
    };

    // Validate filename
    if filename.is_empty() || !filename.ends_with(".md") {
        println!("Error: Invalid filename: {filename}");
        return Ok(());
    }

    // Content is already extracted by the wrapper

    // Get the commands directory
    let commands_dir = get_commands_dir(&Scope::Project)?;
    fs::create_dir_all(&commands_dir)?;

    // Write the file
    let output_path = commands_dir.join(filename);
    fs::write(&output_path, content)?;

    println!("[OK] Created command: {}", output_path.display());

    Ok(())
}
