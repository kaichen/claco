use anyhow::{anyhow, Context, Result};
use claco::claude::{load_settings, project_settings_path, save_settings, user_settings_path, Settings};
use claco::cli::{Scope, SettingsSubcommand};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Handle the settings subcommand
pub async fn handle_settings(cmd: SettingsSubcommand) -> Result<()> {
    match cmd {
        SettingsSubcommand::Apply { source, scope, overwrite } => {
            apply_settings(&source, scope, overwrite).await
        }
    }
}

/// Apply settings from a source file or URL
async fn apply_settings(source: &str, scope: Scope, overwrite: bool) -> Result<()> {
    // Get the source settings
    let source_settings = load_source_settings(source).await?;
    
    // Get the target settings path
    let target_path = match scope {
        Scope::User => user_settings_path()?,
        Scope::Project => project_settings_path(),
    };
    
    // Load existing settings
    let mut target_settings = load_settings(&target_path)?;
    
    // Merge settings
    merge_settings(&mut target_settings, source_settings, overwrite)?;
    
    // Save the merged settings
    save_settings(&target_path, &target_settings)?;
    
    println!("Successfully applied settings to {} scope", match scope {
        Scope::User => "user",
        Scope::Project => "project",
    });
    
    Ok(())
}

/// Load settings from a source (file path or GitHub URL)
async fn load_source_settings(source: &str) -> Result<Settings> {
    if source.starts_with("https://github.com/") {
        // Handle GitHub URL
        load_from_github_url(source).await
    } else {
        // Handle local file
        load_from_local_file(source)
    }
}

/// Load settings from a GitHub URL
async fn load_from_github_url(url: &str) -> Result<Settings> {
    // Convert GitHub URL to raw content URL
    let raw_url = convert_to_raw_github_url(url)?;
    
    // Fetch content from GitHub
    let response = reqwest::get(&raw_url)
        .await
        .with_context(|| format!("Failed to fetch settings from GitHub URL: {}", url))?;
    
    if !response.status().is_success() {
        return Err(anyhow!("Failed to fetch settings from GitHub: {}", response.status()));
    }
    
    let content = response.text()
        .await
        .context("Failed to read response body")?;
    
    // Parse the JSON content
    serde_json::from_str::<Settings>(&content)
        .with_context(|| format!("Failed to parse settings JSON from GitHub URL: {}", url))
}

/// Convert a GitHub URL to raw content URL
fn convert_to_raw_github_url(url: &str) -> Result<String> {
    // Convert https://github.com/owner/repo/blob/branch/path
    // to https://raw.githubusercontent.com/owner/repo/branch/path
    
    if !url.starts_with("https://github.com/") {
        return Err(anyhow!("Invalid GitHub URL format"));
    }
    
    let parts: Vec<&str> = url.trim_start_matches("https://github.com/").split('/').collect();
    
    if parts.len() < 5 || parts[2] != "blob" {
        return Err(anyhow!("Invalid GitHub URL format. Expected: https://github.com/owner/repo/blob/branch/path"));
    }
    
    let owner = parts[0];
    let repo = parts[1];
    let branch = parts[3];
    let path = parts[4..].join("/");
    
    Ok(format!("https://raw.githubusercontent.com/{}/{}/{}/{}", owner, repo, branch, path))
}

/// Load settings from a local file
fn load_from_local_file(path: &str) -> Result<Settings> {
    let path = PathBuf::from(path);
    
    if !path.exists() {
        return Err(anyhow!("Settings file not found: {}", path.display()));
    }
    
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read settings file: {}", path.display()))?;
    
    serde_json::from_str::<Settings>(&content)
        .with_context(|| format!("Failed to parse settings JSON from file: {}", path.display()))
}

/// Merge source settings into target settings
fn merge_settings(target: &mut Settings, source: Settings, overwrite: bool) -> Result<()> {
    // Check for conflicts if not overwriting
    if !overwrite {
        check_for_conflicts(target, &source)?;
    }
    
    // Merge hooks
    if let Some(source_hooks) = source.hooks {
        if target.hooks.is_none() {
            target.hooks = Some(HashMap::new());
        }
        
        let target_hooks = target.hooks.as_mut().unwrap();
        
        for (event, matchers) in source_hooks {
            if overwrite {
                target_hooks.insert(event, matchers);
            } else {
                // Append matchers for existing events
                target_hooks.entry(event).or_insert_with(Vec::new).extend(matchers);
            }
        }
    }
    
    // Merge other fields
    for (key, value) in source.other {
        if overwrite || !target.other.contains_key(&key) {
            target.other.insert(key, value);
        }
    }
    
    Ok(())
}

/// Check for conflicts between target and source settings
fn check_for_conflicts(target: &Settings, source: &Settings) -> Result<()> {
    let mut conflicts = Vec::new();
    
    // Check for conflicting hooks
    if let (Some(target_hooks), Some(source_hooks)) = (&target.hooks, &source.hooks) {
        for event in source_hooks.keys() {
            if target_hooks.contains_key(event) {
                conflicts.push(format!("Hook event: {}", event));
            }
        }
    }
    
    // Check for conflicting other fields
    for key in source.other.keys() {
        if target.other.contains_key(key) {
            conflicts.push(format!("Setting: {}", key));
        }
    }
    
    if !conflicts.is_empty() {
        return Err(anyhow!(
            "Conflicts detected in the following settings:\n{}\n\nUse --overwrite to replace existing settings",
            conflicts.join("\n")
        ));
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_convert_to_raw_github_url() {
        let url = "https://github.com/kaichen/dot-claude/blob/main/.claude/settings.permissions.json";
        let raw_url = convert_to_raw_github_url(url).unwrap();
        assert_eq!(raw_url, "https://raw.githubusercontent.com/kaichen/dot-claude/main/.claude/settings.permissions.json");
    }
    
    #[test]
    fn test_invalid_github_url() {
        let url = "https://github.com/invalid/url";
        assert!(convert_to_raw_github_url(url).is_err());
    }
}