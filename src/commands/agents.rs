use anyhow::Result;
use claco::{claude_home, AgentsSubcommand, Scope};
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct AgentInfo {
    name: String,
    description: String,
    tools: Option<Vec<String>>,
    color: Option<String>,
}

pub async fn handle_agents(cmd: AgentsSubcommand) -> Result<()> {
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

fn list_agents_recursive(dir: &std::path::Path, namespace: &str, scope: &Scope) -> Result<()> {
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
            list_agents_recursive(&path, &new_namespace, scope)?;
        } else if file_name_str.ends_with(".md") {
            let agent_name = file_name_str.strip_suffix(".md").unwrap();
            let full_agent_name = if namespace.is_empty() {
                agent_name.to_string()
            } else {
                format!("{namespace}/{agent_name}")
            };

            // Try to read and parse agent metadata
            if let Ok(content) = fs::read_to_string(&path) {
                if let Some(agent_info) = parse_agent_metadata(&content) {
                    let scope_label = match scope {
                        Scope::User => "(user)",
                        Scope::Project => "(project)",
                    };
                    // Truncate long descriptions for display
                    let description = if agent_info.description.len() > 80 {
                        format!("{}...", &agent_info.description[..77])
                    } else {
                        agent_info.description.clone()
                    };
                    println!(
                        "- {} {} - {} [{}]",
                        full_agent_name, scope_label, agent_info.name, description
                    );
                } else {
                    let scope_label = match scope {
                        Scope::User => "(user)",
                        Scope::Project => "(project)",
                    };
                    println!("- {full_agent_name} {scope_label} - (no metadata)");
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
    let api_path = format!("repos/{owner}/{repo}/contents/{file_path}?ref={branch}");

    let output = Command::new("gh")
        .args(["api", &api_path, "--jq", ".content"])
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to download agent: {}", error);
    }

    // Decode base64 content
    let base64_content = String::from_utf8(output.stdout)?;
    // GitHub returns base64 with newlines, we need to remove all whitespace
    let base64_content: String = base64_content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    use base64::{engine::general_purpose, Engine as _};
    let content = general_purpose::STANDARD.decode(&base64_content)?;
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
        println!("Importing agent: {}", agent_info.name);
        println!("Description: {}", agent_info.description);
        if let Some(ref tools) = agent_info.tools {
            println!("Tools: {}", tools.join(", "));
        }
        if let Some(ref color) = agent_info.color {
            println!("Color: {color}");
        }
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
            let agent_name = file_name_str.strip_suffix(".md").unwrap();
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
    println!("✓ Removed {agent_count} agent(s)");

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
    println!("Launching Claude to generate agent...");

    // System prompt for agent generation
    let system_prompt = r#"You are an agent generator for Claude Code. Generate markdown files for custom agents following this format:

STRUCTURE:
- Store in .claude/agents/ directory
- Filename: agent-name.md (descriptive name)
- Required YAML frontmatter with:
  - name: descriptive-name-of-agent
  - description: Brief description of when to use this agent
  - tools: (optional) Comma-separated list of tools like Read, Edit, Bash
  - color: (optional) Color for the agent display

CONTENT:
- Clear instructions for the agent's specialized behavior
- Define the agent's expertise and approach
- Include specific guidelines and best practices
- Use second person ("You are...") for instructions

EXAMPLE:
```markdown
---
name: security-analyst
description: Security vulnerability analysis, threat modeling, and security best practices recommendations
tools: Read, Grep, Edit
color: red
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
    let mut cmd = Command::new("claude");
    cmd.arg("--no-color")
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
}
