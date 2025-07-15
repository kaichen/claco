use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Represents a single entry in a Claude session JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "isSidechain")]
    pub is_sidechain: bool,
    #[serde(rename = "userType")]
    pub user_type: String,
    pub cwd: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub version: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub message: Message,
    pub uuid: String,
    pub timestamp: String,
}

/// Represents a message in the Claude session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Represents a lock file in the ide directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    pub pid: u32,
    #[serde(rename = "workspaceFolders")]
    pub workspace_folders: Vec<String>,
    #[serde(rename = "ideName")]
    pub ide_name: String,
    pub transport: String,
    #[serde(rename = "authToken")]
    pub auth_token: String,
}

/// Represents a single hook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub command: String,
}

/// Represents a hook matcher with its associated hooks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    pub matcher: String,
    pub hooks: Vec<Hook>,
}

/// Represents the hooks section in settings.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Hooks {
    #[serde(flatten)]
    pub events: HashMap<String, Vec<HookMatcher>>,
}

/// Represents a Claude settings.json file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Hooks>,
}

/// Get the Claude home directory
pub fn claude_home() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".claude")
}

/// Convert a working directory path to a sanitized project directory name
pub fn sanitize_project_path(cwd: &str) -> String {
    let mut result = String::new();
    let mut last_was_separator = false;

    for c in cwd.trim_start_matches('/').chars() {
        if c == '\\' || c == '/' || c == ':' {
            if !last_was_separator && !result.is_empty() {
                result.push('-');
            }
            last_was_separator = true;
        } else {
            result.push(c);
            last_was_separator = false;
        }
    }

    result.trim_matches('-').to_string()
}

/// Convert a sanitized project directory name back to the original path
pub fn desanitize_project_path(sanitized: &str) -> String {
    format!("/{}", sanitized.replace('-', "/"))
}

/// Get the path to a project's directory in ~/.claude/projects
pub fn project_dir(cwd: &str) -> PathBuf {
    let sanitized = sanitize_project_path(cwd);
    claude_home().join("projects").join(sanitized)
}

/// Get the path to the ide directory
pub fn ide_dir() -> PathBuf {
    claude_home().join("ide")
}

/// Get the path to user settings.json
pub fn user_settings_path() -> PathBuf {
    claude_home().join("settings.json")
}

/// Get the path to project settings.json
pub fn project_settings_path() -> PathBuf {
    PathBuf::from(".claude").join("settings.json")
}

/// Load settings from a file path
pub fn load_settings(path: &PathBuf) -> anyhow::Result<Settings> {
    use std::fs;

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = fs::read_to_string(path)?;
    let settings: Settings = serde_json::from_str(&content)?;
    Ok(settings)
}

/// Save settings to a file path
pub fn save_settings(path: &PathBuf, settings: &Settings) -> anyhow::Result<()> {
    use std::fs;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(settings)?;
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_project_path() {
        assert_eq!(
            sanitize_project_path("/Users/kaichen/workspace/claco"),
            "Users-kaichen-workspace-claco"
        );
        assert_eq!(sanitize_project_path("///Users///test//"), "Users-test");
    }

    #[test]
    fn test_desanitize_project_path() {
        assert_eq!(
            desanitize_project_path("Users-kaichen-workspace-claco"),
            "/Users/kaichen/workspace/claco"
        );
    }
}
