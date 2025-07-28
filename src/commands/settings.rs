use anyhow::{anyhow, Context, Result};
use claco::claude::{
    load_settings, project_local_settings_path, project_settings_path, save_settings,
    user_settings_path, Settings,
};
use claco::cli::{Scope, SettingsSubcommand};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Format JSON parsing errors with line/column information
fn format_json_error(err: &serde_json::Error, content: &str) -> String {
    let line_num = err.line();
    let col_num = err.column();

    // Line and column numbers are 1-based and always available for parsing errors
    if line_num > 0 {
        let lines: Vec<&str> = content.lines().collect();
        let mut error_msg =
            format!("JSON parsing error at line {line_num}, column {col_num}: {err}");

        // Show the problematic line with context
        if line_num <= lines.len() {
            error_msg.push_str("\n\n");

            // Show previous line if available
            if line_num > 1 {
                error_msg.push_str(&format!("{:4}: {}\n", line_num - 1, lines[line_num - 2]));
            }

            // Show the error line
            error_msg.push_str(&format!("{:4}: {}\n", line_num, lines[line_num - 1]));

            // Show pointer to the error column
            if col_num > 0 {
                error_msg.push_str(&format!("      {}^\n", " ".repeat(col_num - 1)));
            }

            // Show next line if available
            if line_num < lines.len() {
                error_msg.push_str(&format!("{:4}: {}\n", line_num + 1, lines[line_num]));
            }
        }

        error_msg
    } else {
        format!("JSON parsing error: {err}")
    }
}

/// Handle the settings subcommand
pub async fn handle_settings(cmd: SettingsSubcommand) -> Result<()> {
    match cmd {
        SettingsSubcommand::Apply {
            source,
            scope,
            overwrite,
        } => apply_settings(&source, scope, overwrite).await,
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
        Scope::ProjectLocal => project_local_settings_path(),
    };

    // Load existing settings
    // NOTE: There is a race condition window between loading and saving settings.
    // If two processes run `settings apply` simultaneously, one could overwrite
    // the other's changes. This is a known limitation. For most use cases this
    // is acceptable since settings modifications are typically infrequent.
    let mut target_settings = load_settings(&target_path)?;

    // Merge settings
    merge_settings(&mut target_settings, source_settings, overwrite)?;

    // Save the merged settings
    save_settings(&target_path, &target_settings)?;

    println!(
        "Successfully applied settings to {} scope",
        match scope {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::ProjectLocal => "project.local",
        }
    );

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

    // Create HTTP client with timeout
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Fetch content from GitHub
    let response = client
        .get(&raw_url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch settings from GitHub URL: {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch settings from GitHub: {}",
            response.status()
        ));
    }

    let content = response
        .text()
        .await
        .context("Failed to read response body")?;

    // Parse the JSON content
    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            let error_msg = format_json_error(&e, &content);
            Err(anyhow!(
                "Failed to parse settings JSON from GitHub URL: {url}\n{error_msg}"
            ))
        }
    }
}

/// Convert a GitHub URL to raw content URL
fn convert_to_raw_github_url(url: &str) -> Result<String> {
    // Convert https://github.com/owner/repo/blob/branch/path
    // to https://raw.githubusercontent.com/owner/repo/branch/path

    if !url.starts_with("https://github.com/") {
        return Err(anyhow!("Invalid GitHub URL format"));
    }

    let parts: Vec<&str> = url
        .trim_start_matches("https://github.com/")
        .split('/')
        .collect();

    if parts.len() < 5 || parts[2] != "blob" {
        return Err(anyhow!(
            "Invalid GitHub URL format. Expected: https://github.com/owner/repo/blob/branch/path"
        ));
    }

    let owner = parts[0];
    let repo = parts[1];
    let branch = parts[3];
    let path = parts[4..].join("/");

    // Ensure the path is not empty after joining
    if path.is_empty() {
        return Err(anyhow!("Invalid GitHub URL: empty file path"));
    }

    Ok(format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}"
    ))
}

/// Load settings from a local file
fn load_from_local_file(path: &str) -> Result<Settings> {
    let path = PathBuf::from(path);

    // Canonicalize the path to resolve symlinks and relative paths
    // This helps prevent some path traversal attacks
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("Failed to resolve path: {}", path.display()))?;

    // Security note: This tool should only be used with trusted input.
    // The settings file could potentially contain sensitive configuration.
    // Consider validating that the path is within expected directories
    // if this tool will be used in untrusted environments.

    let content = fs::read_to_string(&canonical_path)
        .with_context(|| format!("Failed to read settings file: {}", canonical_path.display()))?;

    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            let error_msg = format_json_error(&e, &content);
            Err(anyhow!(
                "Failed to parse settings JSON from file: {}\n{}",
                canonical_path.display(),
                error_msg
            ))
        }
    }
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
                target_hooks.entry(event).or_default().extend(matchers);
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
                conflicts.push(format!("$.hooks.{event}"));
            }
        }
    }

    // Check for conflicting other fields
    for key in source.other.keys() {
        if target.other.contains_key(key) {
            // Build JSONPath for the key
            let jsonpath = format!("$.{key}");
            conflicts.push(jsonpath);
        }
    }

    if !conflicts.is_empty() {
        return Err(anyhow!(
            "Conflicts detected at the following paths:\n{}\n\nUse --overwrite to replace existing settings",
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
        let url =
            "https://github.com/kaichen/dot-claude/blob/main/.claude/settings.permissions.json";
        let raw_url = convert_to_raw_github_url(url).unwrap();
        assert_eq!(raw_url, "https://raw.githubusercontent.com/kaichen/dot-claude/main/.claude/settings.permissions.json");
    }

    #[test]
    fn test_invalid_github_url() {
        let url = "https://github.com/invalid/url";
        assert!(convert_to_raw_github_url(url).is_err());
    }

    #[test]
    fn test_github_url_without_file_path() {
        let url = "https://github.com/owner/repo/blob/main";
        assert!(convert_to_raw_github_url(url).is_err());
    }

    #[test]
    fn test_github_url_with_query_params() {
        let url = "https://github.com/owner/repo/blob/main/file.json?ref=feature";
        let result = convert_to_raw_github_url(url).unwrap();
        // The query params will be included in the path - this is expected behavior
        assert_eq!(
            result,
            "https://raw.githubusercontent.com/owner/repo/main/file.json?ref=feature"
        );
    }
}
