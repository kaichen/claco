use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Represents a single entry in a Claude session JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "isSidechain", skip_serializing_if = "Option::is_none")]
    pub is_sidechain: Option<bool>,
    #[serde(rename = "userType")]
    pub user_type: Option<String>,
    pub cwd: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "type")]
    pub message_type: String,
    pub message: Option<Message>,
    pub uuid: Option<String>,
    pub timestamp: Option<String>,
}

/// Represents a message in the Claude session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(deserialize_with = "deserialize_content")]
    pub content: String,
}

/// Custom deserializer for message content that can be either a string or an array
fn deserialize_content<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use serde_json::Value;

    struct ContentVisitor;

    impl<'de> Visitor<'de> for ContentVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or an array of content objects")
        }

        fn visit_str<E>(self, value: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_string<E>(self, value: String) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<String, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut result = String::new();
            while let Some(value) = seq.next_element::<Value>()? {
                if let Some(obj) = value.as_object() {
                    if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(text);
                    }
                }
            }
            Ok(result)
        }
    }

    deserializer.deserialize_any(ContentVisitor)
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
/// This is directly a HashMap of event names to matchers
pub type Hooks = HashMap<String, Vec<HookMatcher>>;

/// Represents a Claude settings.json file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Hooks>,

    // Preserve all other fields from the settings.json file
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

/// Get the Claude home directory
pub fn claude_home() -> Result<PathBuf> {
    dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
        .map(|home| home.join(".claude"))
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
pub fn project_dir(cwd: &str) -> Result<PathBuf> {
    let sanitized = sanitize_project_path(cwd);
    Ok(claude_home()?.join("projects").join(sanitized))
}

/// Get the path to user settings.json
pub fn user_settings_path() -> Result<PathBuf> {
    Ok(claude_home()?.join("settings.json"))
}

/// Get the path to project settings.json
pub fn project_settings_path() -> PathBuf {
    PathBuf::from(".claude").join("settings.json")
}

/// Get the path to project-local settings
pub fn project_local_settings_path() -> PathBuf {
    PathBuf::from(".claude").join("settings.local.json")
}

/// Load settings from a file path
pub fn load_settings(path: &PathBuf) -> anyhow::Result<Settings> {
    use anyhow::Context;

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read settings file: {}", path.display()))?;

    // Try to parse as-is first
    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(_) => {
            // Try to parse as raw JSON and migrate old format
            let mut value: Value = serde_json::from_str(&content).with_context(|| {
                format!("Failed to parse settings JSON from: {}", path.display())
            })?;

            // Check if we need to fix missing "type" fields in hooks
            if let Some(hooks_value) = value.get_mut("hooks") {
                if let Some(hooks_obj) = hooks_value.as_object_mut() {
                    // Check if this has the old "events" wrapper that needs to be removed
                    if let Some(events) = hooks_obj.get("events") {
                        // Old format with "events" wrapper - unwrap it
                        *hooks_value = events.clone();
                    }

                    // Now fix missing "type" fields
                    if let Some(events_obj) = hooks_value.as_object_mut() {
                        for (_, matchers) in events_obj {
                            if let Some(matchers_array) = matchers.as_array_mut() {
                                for matcher in matchers_array {
                                    if let Some(matcher_obj) = matcher.as_object_mut() {
                                        // Fix hooks array to ensure each hook has a "type" field
                                        if let Some(hooks_array) = matcher_obj.get_mut("hooks") {
                                            if let Some(hooks) = hooks_array.as_array_mut() {
                                                for hook in hooks {
                                                    if let Some(hook_obj) = hook.as_object_mut() {
                                                        if !hook_obj.contains_key("type") {
                                                            hook_obj.insert(
                                                                "type".to_string(),
                                                                Value::String(
                                                                    "command".to_string(),
                                                                ),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Now try to parse the migrated value
            serde_json::from_value(value)
                .with_context(|| format!("Failed to parse settings from: {}", path.display()))
        }
    }
}

/// Save settings to a file path with atomic operations
pub fn save_settings(path: &PathBuf, settings: &Settings) -> anyhow::Result<()> {
    use anyhow::Context;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    }

    // Serialize to JSON first to validate
    let content =
        serde_json::to_string_pretty(settings).context("Failed to serialize settings to JSON")?;

    // Create a temporary file in the same directory
    let temp_path = path.with_extension("tmp");

    // Clean up temp file if it exists from a previous failed attempt
    if temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    // Write to temporary file with error handling
    let result = (|| -> anyhow::Result<()> {
        let mut temp_file = fs::File::create(&temp_path)
            .with_context(|| format!("Failed to create temporary file: {}", temp_path.display()))?;

        temp_file
            .write_all(content.as_bytes())
            .context("Failed to write settings to temporary file")?;

        temp_file
            .sync_all()
            .context("Failed to sync temporary file to disk")?;

        // Atomically rename temp file to target
        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to save settings to: {}", path.display()))?;

        Ok(())
    })();

    // Clean up temp file on error
    if result.is_err() && temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }

    result
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

    #[test]
    fn test_settings_preserve_unknown_fields() {
        use serde_json::json;

        // JSON with known and unknown fields
        let json_str = r#"{
            "hooks": {},
            "model": "claude-3-opus",
            "cleanupPeriodDays": 30,
            "customField": "test"
        }"#;

        let settings: Settings = serde_json::from_str(json_str).unwrap();

        // Check that hooks field is parsed correctly
        assert!(settings.hooks.is_some());

        // Check that unknown fields are preserved
        assert_eq!(settings.other.get("model"), Some(&json!("claude-3-opus")));
        assert_eq!(settings.other.get("cleanupPeriodDays"), Some(&json!(30)));
        assert_eq!(settings.other.get("customField"), Some(&json!("test")));

        // Serialize back and check all fields are preserved
        let serialized = serde_json::to_value(&settings).unwrap();
        assert_eq!(serialized["model"], json!("claude-3-opus"));
        assert_eq!(serialized["cleanupPeriodDays"], json!(30));
        assert_eq!(serialized["customField"], json!("test"));
    }

    #[test]
    fn test_save_and_load_settings() {
        use serde_json::json;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        // Create settings with hooks and other fields
        let mut settings = Settings {
            hooks: Some(HashMap::new()),
            ..Default::default()
        };
        settings
            .other
            .insert("model".to_string(), json!("claude-3-opus"));
        settings
            .other
            .insert("cleanupPeriodDays".to_string(), json!(7));

        // Save settings
        save_settings(&settings_path, &settings).unwrap();

        // Load settings back
        let loaded = load_settings(&settings_path).unwrap();

        // Verify fields are preserved
        assert!(loaded.hooks.is_some());
        assert_eq!(loaded.other.get("model"), Some(&json!("claude-3-opus")));
        assert_eq!(loaded.other.get("cleanupPeriodDays"), Some(&json!(7)));
    }

    #[test]
    fn test_atomic_save_cleanup() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        // Create initial settings file
        let settings = Settings::default();
        save_settings(&settings_path, &settings).unwrap();

        // Create a temp file manually to simulate a previous failed attempt
        let temp_path = settings_path.with_extension("tmp");
        fs::write(&temp_path, "incomplete").unwrap();

        // Save should clean up the existing temp file and succeed
        let result = save_settings(&settings_path, &settings);
        assert!(result.is_ok());

        // Verify temp file was cleaned up
        assert!(!temp_path.exists());

        // Verify final file is valid
        assert!(settings_path.exists());
    }

    #[test]
    fn test_load_corrupted_json() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        // Write corrupted JSON
        fs::write(&settings_path, "{ invalid json }").unwrap();

        // Try to load
        let result = load_settings(&settings_path);

        // Should return error with helpful message
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to parse settings JSON"));
        assert!(err_msg.contains(settings_path.display().to_string().as_str()));
    }

    #[test]
    fn test_concurrent_settings_modification() {
        use serde_json::json;
        use std::sync::{Arc, Barrier};
        use std::thread;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let settings_path = Arc::new(dir.path().join("settings.json"));

        // Initial settings
        let initial = Settings::default();
        save_settings(&settings_path, &initial).unwrap();

        // Use barrier to synchronize threads
        let barrier = Arc::new(Barrier::new(2));
        let path1 = settings_path.clone();
        let path2 = settings_path.clone();
        let barrier1 = barrier.clone();
        let barrier2 = barrier.clone();

        // Thread 1: Add hooks
        let handle1 = thread::spawn(move || {
            barrier1.wait();
            let mut settings = load_settings(&path1).unwrap();
            settings.hooks = Some(HashMap::new());
            save_settings(&path1, &settings)
        });

        // Thread 2: Add other fields
        let handle2 = thread::spawn(move || {
            barrier2.wait();
            let mut settings = load_settings(&path2).unwrap();
            settings
                .other
                .insert("model".to_string(), json!("claude-3-opus"));
            save_settings(&path2, &settings)
        });

        // Both should succeed due to atomic operations
        let result1 = handle1.join().unwrap();
        let result2 = handle2.join().unwrap();

        // At least one should succeed
        assert!(result1.is_ok() || result2.is_ok());

        // Wait a bit for filesystem to settle
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Final file should be valid JSON
        let final_settings = load_settings(&settings_path);
        assert!(final_settings.is_ok());
    }
}
