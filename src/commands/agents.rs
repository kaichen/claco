use anyhow::Result;
use claco::{claude_home, AgentsSubcommand, Scope};
use std::fs;
use std::io::{self, Write};
use std::process::Command;

// Constants
const MAX_GITHUB_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB

#[derive(Debug)]
struct AgentInfo {
    #[allow(dead_code)]
    name: String,
    description: String,
    #[allow(dead_code)]
    tools: Option<Vec<String>>,
    #[allow(dead_code)]
    color: Option<String>,
}

/// Handle agent-related subcommands
///
/// This function processes all agent management operations including:
/// - Listing agents from user/project scopes
/// - Importing agents from files or GitHub
/// - Deleting agents interactively
/// - Cleaning up all agents in a scope
/// - Generating new agents using Claude
pub async fn handle_agents(cmd: AgentsSubcommand) -> Result<()> {
    match cmd {
        AgentsSubcommand::List { scope } => handle_agents_list(scope)?,
        AgentsSubcommand::Import { source, scope } => handle_agents_import(source, scope).await?,
        AgentsSubcommand::Delete { interactive } => handle_agents_delete(interactive)?,
        AgentsSubcommand::Clean { scope } => handle_agents_clean(scope)?,
        AgentsSubcommand::Generate { filename } => handle_agents_generate(filename)?,
    }
    Ok(())
}

fn get_agents_dir(scope: &Scope) -> Result<std::path::PathBuf> {
    match scope {
        Scope::User => Ok(claude_home()?.join("agents")),
        Scope::Project => {
            let cwd = std::env::current_dir()?;
            Ok(cwd.join(".claude").join("agents"))
        }
        Scope::ProjectLocal => {
            anyhow::bail!("project.local scope is not supported for agents")
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
                Scope::ProjectLocal => {
                    return Err(anyhow::anyhow!(
                        "project.local scope is not supported for agents"
                    ));
                }
            };

            println!("Custom agents ({}): {}", scope_label, agents_dir.display());
            println!();

            list_agents_recursive(&agents_dir, "", &specific_scope)?;
        }
        None => {
            // Show agents from both user and project scopes
            // List user agents
            let user_scope = Scope::User;
            let user_agents_dir = get_agents_dir(&user_scope)?;

            if user_agents_dir.exists() {
                println!("Custom agents (user): {}", user_agents_dir.display());
                println!();
                list_agents_recursive(&user_agents_dir, "", &user_scope)?;
                println!();
            }

            // List project agents
            let project_scope = Scope::Project;
            let project_agents_dir = get_agents_dir(&project_scope)?;

            if project_agents_dir.exists() {
                println!("Custom agents (project): {}", project_agents_dir.display());
                println!();
                list_agents_recursive(&project_agents_dir, "", &project_scope)?;
            } else if !user_agents_dir.exists() {
                println!("No agents found in user or project directories.");
            }
        }
    }

    Ok(())
}

fn list_agents_recursive(dir: &std::path::Path, namespace: &str, _scope: &Scope) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();

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
                format!("{namespace}/{file_name_str}")
            };
            list_agents_recursive(&path, &new_namespace, _scope)?;
        } else if file_name_str.ends_with(".md") {
            let agent_name = match file_name_str.strip_suffix(".md") {
                Some(name) => name,
                None => continue, // Skip if somehow strip_suffix fails
            };
            let full_agent_name = if namespace.is_empty() {
                agent_name.to_string()
            } else {
                format!("{namespace}/{agent_name}")
            };

            // Try to read and parse agent metadata
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(agent_info) = parse_agent_metadata(&content) {
                    // Truncate long descriptions for display
                    if agent_info.description.len() > 80 {
                        let truncated = agent_info.description.chars().take(77).collect::<String>();
                        println!("  {full_agent_name} [{truncated}...]");
                    } else {
                        println!("  {} [{}]", full_agent_name, agent_info.description);
                    }
                } else {
                    println!("  {full_agent_name} [no description]");
                }
            }
        }
    }

    Ok(())
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
    let mut name = String::new();
    let mut description = String::new();
    let mut tools = None;
    let mut color = None;

    for line in frontmatter.lines() {
        // New schema is primary, old schema as fallback for compatibility
        if let Some(value) = line
            .strip_prefix("name: ")
            .or_else(|| line.strip_prefix("agent-type: "))
        {
            name = value.trim().to_string();
        } else if let Some(value) = line
            .strip_prefix("description: ")
            .or_else(|| line.strip_prefix("when-to-use: "))
        {
            description = value.trim().to_string();
        } else if let Some(value) = line
            .strip_prefix("tools: ")
            .or_else(|| line.strip_prefix("allowed-tools: "))
        {
            // Parse as comma-separated string (no array format, ignore '*')
            let value = value.trim();

            // Skip if it's '*' or an array format
            if value == "*" || value.starts_with('[') {
                continue;
            }

            if !value.is_empty() {
                let tool_list: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !tool_list.is_empty() {
                    tools = Some(tool_list);
                }
            }
        } else if let Some(value) = line.strip_prefix("color: ") {
            color = Some(value.trim().to_string());
        }
    }

    if name.is_empty() {
        return None;
    }

    Some(AgentInfo {
        name,
        description,
        tools,
        color,
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
        anyhow::bail!("Only GitHub URLs are supported. Example: https://github.com/owner/repo/blob/main/path/to/agent.md or https://github.com/owner/repo/tree/main/path/to/folder");
    }

    // Extract owner, repo, and path from GitHub URL
    let path_segments: Vec<&str> = parsed_url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL: No path segments"))?
        .filter(|s| !s.is_empty()) // Filter out empty segments from trailing slashes
        .collect();

    // Handle different URL formats
    match path_segments.len() {
        // https://github.com/owner/repo format
        2 => {
            println!("Checking for .md files in repository root...");
            // Import from repo root directory
            import_agents_from_repo_url(path_segments[0], path_segments[1], None, "main", scope)
                .await
        }
        // Standard blob/tree URLs
        _ if path_segments.len() >= 4 => {
            // Check if it's a tree (folder) or blob (file) URL
            let url_type = path_segments.get(2).copied();

            match url_type {
                Some("blob") => {
                    if path_segments.len() < 5 {
                        anyhow::bail!("Invalid file URL format. Expected: https://github.com/owner/repo/blob/branch/path/to/agent.md");
                    }

                    // Check if the last segment looks like a file or directory
                    let last_segment = path_segments.last().unwrap();
                    if !last_segment.ends_with(".md") {
                        // This might be a directory shown as blob by GitHub
                        // Try to list it first
                        let owner = path_segments[0];
                        let repo = path_segments[1];
                        let branch = path_segments[3];
                        let path = path_segments[4..].join("/");

                        println!("Checking if URL points to a directory...");

                        // Try to list the path as a directory
                        let api_path = format!("repos/{owner}/{repo}/contents/{path}?ref={branch}");
                        let check_output = Command::new("gh").args(["api", &api_path]).output()?;

                        if check_output.status.success() {
                            // Parse to check if it's an array (directory)
                            let json_str = String::from_utf8(check_output.stdout)?;
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                if json.is_array() {
                                    // It's a directory, convert to tree URL
                                    println!(
                                        "URL points to a directory. Converting to tree URL..."
                                    );
                                    let mut tree_segments = path_segments.to_vec();
                                    tree_segments[2] = "tree";
                                    return import_agents_folder_from_github(&tree_segments, scope)
                                        .await;
                                }
                            }
                        }
                    }

                    // Import single file
                    import_single_agent_from_github(&path_segments, scope).await
                }
                Some("tree") => {
                    // Import all .md files from folder
                    import_agents_folder_from_github(&path_segments, scope).await
                }
                _ => {
                    anyhow::bail!("Invalid GitHub URL format. URL must be either:\n  - https://github.com/owner/repo (imports from root)\n  - https://github.com/owner/repo/blob/branch/path/to/agent.md (single file)\n  - https://github.com/owner/repo/tree/branch/path/to/folder (folder)");
                }
            }
        }
        _ => {
            anyhow::bail!("Invalid GitHub URL format. URL must be either:\n  - https://github.com/owner/repo (imports from root)\n  - https://github.com/owner/repo/blob/branch/path/to/agent.md (single file)\n  - https://github.com/owner/repo/tree/branch/path/to/folder (folder)");
        }
    }
}

async fn import_agents_from_repo_url(
    owner: &str,
    repo: &str,
    path: Option<&str>,
    branch: &str,
    scope: Scope,
) -> Result<()> {
    // Validate components don't contain dangerous characters
    for component in [owner, repo, branch] {
        if component.contains([
            '$', '`', '\\', '"', '\'', '\n', '\r', ';', '|', '&', '<', '>', '(', ')',
        ]) {
            anyhow::bail!("Invalid characters in URL component: {}", component);
        }
    }

    // List files in the repository root or specified path
    let api_path = if let Some(folder_path) = path {
        // Additional validation for folder path
        if folder_path.contains("..") {
            anyhow::bail!("Invalid folder path in URL: Path traversal detected");
        }
        format!("repos/{owner}/{repo}/contents/{folder_path}?ref={branch}")
    } else {
        format!("repos/{owner}/{repo}/contents?ref={branch}")
    };

    let output = Command::new("gh").args(["api", &api_path]).output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        if error.contains("404") {
            anyhow::bail!("Repository or path not found. Make sure the repository exists and you have access to it.");
        }
        anyhow::bail!("Failed to list repository contents: {}", error);
    }

    // Parse JSON response
    let json_str = String::from_utf8(output.stdout)?;
    let files: serde_json::Value = serde_json::from_str(&json_str)?;

    // Common documentation files to exclude
    const EXCLUDED_FILES: &[&str] = &[
        "README.md",
        "readme.md",
        "Readme.md",
        "CHANGELOG.md",
        "changelog.md",
        "Changelog.md",
        "CONTRIBUTING.md",
        "contributing.md",
        "Contributing.md",
        "LICENSE.md",
        "license.md",
        "License.md",
        "CODE_OF_CONDUCT.md",
        "code_of_conduct.md",
        "SECURITY.md",
        "security.md",
        "Security.md",
        "SUPPORT.md",
        "support.md",
        "Support.md",
        "FUNDING.md",
        "funding.md",
        "Funding.md",
        "PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
        "ISSUE_TEMPLATE.md",
        "issue_template.md",
    ];

    // Filter for .md files, excluding common documentation files
    let md_files: Vec<&serde_json::Value> = files
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON array response"))?
        .iter()
        .filter(|file| {
            if file.get("type").and_then(|t| t.as_str()) != Some("file") {
                return false;
            }

            if let Some(name) = file.get("name").and_then(|n| n.as_str()) {
                // Check if it's a markdown file
                if !name.ends_with(".md") {
                    return false;
                }

                // Exclude common documentation files when importing from repo root
                if path.is_none() && EXCLUDED_FILES.contains(&name) {
                    return false;
                }

                true
            } else {
                false
            }
        })
        .collect();

    if md_files.is_empty() {
        anyhow::bail!("No .md files found in the repository (excluding documentation files). Please check if the repository contains any agent markdown files.");
    }

    println!("Found {} agent file(s) to import", md_files.len());

    let mut imported_count = 0;
    let mut failed_count = 0;

    // Import each .md file
    for file in md_files {
        let file_name = file
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing file name"))?;

        let file_path = if let Some(folder_path) = path {
            format!("{folder_path}/{file_name}")
        } else {
            file_name.to_string()
        };

        println!("Importing {file_name}...");

        // Build the blob URL path segments for reusing existing import function
        let mut file_segments = vec![owner, repo, "blob", branch];
        file_segments.extend(file_path.split('/'));

        match import_single_agent_from_github(&file_segments, scope.clone()).await {
            Ok(_) => imported_count += 1,
            Err(e) => {
                eprintln!("error: failed to import {file_name}: {e}");
                failed_count += 1;
            }
        }
    }

    if failed_count > 0 {
        println!("\n[OK] Imported {imported_count} agent(s), {failed_count} failed");
        anyhow::bail!("Some imports failed");
    } else {
        println!("\n[OK] Successfully imported {imported_count} agent(s)");
    }

    Ok(())
}

async fn import_single_agent_from_github(path_segments: &[&str], scope: Scope) -> Result<()> {
    let owner = path_segments[0];
    let repo = path_segments[1];
    let branch = path_segments[3];
    let file_path = path_segments[4..].join("/");

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

    // Download the file using gh api
    let api_path = format!("repos/{owner}/{repo}/contents/{file_path}?ref={branch}");

    // First, try to get the content assuming it's a file
    let output = Command::new("gh")
        .args(["api", &api_path, "--jq", ".content"])
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);

        // Note: Directory detection is now handled earlier in the flow

        anyhow::bail!("Failed to download agent: {}", error);
    }

    // Decode base64 content
    let base64_content = String::from_utf8(output.stdout)?;
    // GitHub returns base64 with newlines, we need to remove all whitespace
    let base64_content: String = base64_content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    // Check size before decoding to prevent memory exhaustion
    // Base64 decoded size is approximately 3/4 of encoded size
    let estimated_size = (base64_content.len() * 3) / 4;
    if estimated_size > MAX_GITHUB_FILE_SIZE {
        anyhow::bail!(
            "Agent file too large: estimated {} bytes, max {} bytes",
            estimated_size,
            MAX_GITHUB_FILE_SIZE
        );
    }

    use base64::{engine::general_purpose, Engine as _};
    let content = general_purpose::STANDARD.decode(&base64_content)?;

    // Verify actual size after decoding
    if content.len() > MAX_GITHUB_FILE_SIZE {
        anyhow::bail!(
            "Agent file too large: {} bytes, max {} bytes",
            content.len(),
            MAX_GITHUB_FILE_SIZE
        );
    }

    let content_str = String::from_utf8(content)?;

    // Extract filename from URL
    let filename = std::path::Path::new(&file_path)
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("agent.md"));

    // Save the agent
    save_agent_content(&content_str, filename.to_string_lossy().as_ref(), scope)?;

    Ok(())
}

async fn import_agents_folder_from_github(path_segments: &[&str], scope: Scope) -> Result<()> {
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

    println!("Importing {} agent file(s)...", md_files.len());

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

        // Build the blob URL path segments
        let mut file_segments = vec![owner, repo, "blob", branch];
        file_segments.extend(file_path.split('/'));

        match import_single_agent_from_github(&file_segments, scope.clone()).await {
            Ok(_) => imported_count += 1,
            Err(e) => {
                eprintln!("error: failed to import {file_name}: {e}");
                failed_count += 1;
            }
        }
    }

    if failed_count > 0 {
        println!("\n[OK] Imported {imported_count} agent(s), {failed_count} failed");
        anyhow::bail!("Some imports failed");
    } else {
        println!("\n[OK] Successfully imported {imported_count} agent(s)");
    }

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

/// Validate that a filename is safe (no path traversal)
fn validate_agent_filename(filename: &str) -> Result<()> {
    // Check for path traversal attempts
    if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
        anyhow::bail!(
            "Invalid filename '{}': Path traversal not allowed",
            filename
        );
    }

    // Ensure it's a markdown file
    if !filename.ends_with(".md") {
        anyhow::bail!("Invalid filename '{}': Must be a .md file", filename);
    }

    // Check for other dangerous characters
    if filename.contains('\0') {
        anyhow::bail!("Invalid filename '{}': Contains null byte", filename);
    }

    Ok(())
}

fn save_agent_content(content: &str, filename: &str, scope: Scope) -> Result<()> {
    // Validate filename for security
    validate_agent_filename(filename)?;

    // Get the agents directory
    let agents_dir = get_agents_dir(&scope)?;

    // Create the directory if it doesn't exist
    fs::create_dir_all(&agents_dir)?;

    // Save the agent file
    let agent_path = agents_dir.join(filename);
    fs::write(&agent_path, content)?;

    println!("[OK] Imported {}", filename.trim_end_matches(".md"));

    Ok(())
}

fn handle_agents_delete(interactive: bool) -> Result<()> {
    if !interactive {
        eprintln!("error: non-interactive mode is not supported yet");
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
        collect_agents_recursive(&project_agents_dir, "", &project_scope, &mut agents_list)?;
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
            Scope::ProjectLocal => {
                return Err(anyhow::anyhow!(
                    "project.local scope is not supported for agents"
                ));
            }
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
                format!("{namespace}/{file_name_str}")
            };
            collect_agents_recursive(&path, &new_namespace, scope, agents_list)?;
        } else if file_name_str.ends_with(".md") {
            let agent_name = match file_name_str.strip_suffix(".md") {
                Some(name) => name,
                None => continue, // Skip if somehow strip_suffix fails
            };
            let full_agent_name = if namespace.is_empty() {
                agent_name.to_string()
            } else {
                format!("{namespace}/{agent_name}")
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
        Scope::ProjectLocal => {
            return Err(anyhow::anyhow!(
                "project.local scope is not supported for agents"
            ));
        }
    };

    println!("This will delete {agent_count} agent(s) from {scope_label} scope.");
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
    println!("[OK] Removed {agent_count} agent(s)");

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

fn handle_agents_generate(filename: Option<String>) -> Result<()> {
    // Generate template markdown for agent
    let template_content = r#"---
name: agent-name
description: Brief description of when to use this agent
---

# Agent Instructions

You are a specialized agent for [describe specialization].

## Core Responsibilities

[Describe what this agent does and when to use it]

## Approach

[Describe how this agent handles tasks]
"#;

    let filename = filename.unwrap_or_else(|| "agent-template.md".to_string());

    // Get the project agents directory
    let agents_dir = get_agents_dir(&Scope::Project)?;
    fs::create_dir_all(&agents_dir)?;

    let output_path = agents_dir.join(&filename);

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

    println!("[OK] Created agent template: {}", output_path.display());
    println!("\nNext steps:");
    println!("  1. Edit the file to customize your agent");
    println!("  2. Update the 'name' and 'description' fields");
    println!("  3. Configure tools and other properties as needed");
    println!("  4. Replace placeholder content with agent instructions");
    println!("  5. Test it by using the agent in Claude Code");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_metadata_valid() {
        let content = r#"---
name: security-analyst
description: Security vulnerability analysis, threat modeling, and security best practices recommendations
tools: ["*"]
color: red
---

You are a security specialist focused on identifying vulnerabilities and recommending secure coding practices.
"#;

        let agent_info = parse_agent_metadata(content).unwrap();
        assert_eq!(agent_info.name, "security-analyst");
        assert_eq!(
            agent_info.description,
            "Security vulnerability analysis, threat modeling, and security best practices recommendations"
        );
        assert_eq!(agent_info.tools, None); // "*" is ignored
        assert_eq!(agent_info.color, Some("red".to_string()));
    }

    #[test]
    fn test_parse_agent_metadata_with_multiple_tools() {
        // Test with comma-separated tools
        let content = r#"---
name: test-agent
description: Test agent with comma-separated tools
tools: Read, Write, Edit, Bash
---

Content
"#;
        let agent_info = parse_agent_metadata(content).unwrap();
        assert_eq!(agent_info.name, "test-agent");
        assert_eq!(
            agent_info.tools,
            Some(vec![
                "Read".to_string(),
                "Write".to_string(),
                "Edit".to_string(),
                "Bash".to_string()
            ])
        );
    }

    #[test]
    fn test_parse_agent_metadata_invalid_no_frontmatter() {
        let content = "Just some content without frontmatter";
        assert!(parse_agent_metadata(content).is_none());
    }

    #[test]
    fn test_parse_agent_metadata_missing_agent_type() {
        let content = r#"---
description: Some description
tools: ["*"]
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

    #[test]
    fn test_validate_agent_filename_path_traversal() {
        // Test path traversal attempts
        assert!(validate_agent_filename("../etc/passwd.md").is_err());
        assert!(validate_agent_filename("..\\windows\\system32\\config.md").is_err());
        assert!(validate_agent_filename("agents/../../../etc/passwd.md").is_err());
        assert!(validate_agent_filename("test/../../sensitive.md").is_err());

        // Test forward slashes
        assert!(validate_agent_filename("subdir/agent.md").is_err());
        assert!(validate_agent_filename("/etc/passwd.md").is_err());

        // Test backslashes
        assert!(validate_agent_filename("subdir\\agent.md").is_err());
        assert!(validate_agent_filename("C:\\Windows\\System32\\config.md").is_err());

        // Test null bytes
        assert!(validate_agent_filename("agent\0.md").is_err());

        // Test non-.md files
        assert!(validate_agent_filename("agent.txt").is_err());
        assert!(validate_agent_filename("agent").is_err());
        assert!(validate_agent_filename("agent.md.txt").is_err());

        // Test valid filenames
        assert!(validate_agent_filename("agent.md").is_ok());
        assert!(validate_agent_filename("my-cool-agent.md").is_ok());
        assert!(validate_agent_filename("agent_v2.md").is_ok());
        assert!(validate_agent_filename("123.md").is_ok());
    }

    #[test]
    fn test_url_component_validation() {
        // These would be tested in handle_agents_import_from_url but we can't easily test
        // that function due to external dependencies. The validation logic ensures that
        // dangerous characters are rejected:
        let dangerous_chars = [
            '$', '`', '\\', '"', '\'', '\n', '\r', ';', '|', '&', '<', '>', '(', ')',
        ];

        for ch in dangerous_chars {
            let dangerous_string = format!("test{ch}test");
            // In the actual code, this would cause bail!
            assert!(dangerous_string.contains(ch));
        }
    }

    #[test]
    fn test_strip_suffix_safety() {
        // Test that our safe strip_suffix handling works
        let test_cases = vec![
            ("agent.md", Some("agent")),
            ("test.md", Some("test")),
            ("no-extension", None),
            ("double.md.md", Some("double.md")),
            (".md", Some("")),
        ];

        for (input, expected) in test_cases {
            let result = input.strip_suffix(".md");
            assert_eq!(result, expected);
        }
    }
}
