use anyhow::Result;
use claco::{claude_home, desanitize_project_path, SessionEntry};
use std::fs;
use std::io::{BufRead, BufReader};

/// List all Claude Code projects with their sessions
///
/// Reads the ~/.claude/projects directory and displays:
/// - Project paths (desanitized from directory names)
/// - Associated session IDs for each project
/// - Attempts to extract the actual cwd from session files
pub fn handle_projects() -> Result<()> {
    let projects_dir = claude_home()?.join("projects");

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
                    sessions.push(session_id.to_string_lossy().into_owned());
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
            match path.file_name() {
                Some(name) => desanitize_project_path(&name.to_string_lossy()),
                None => {
                    eprintln!(
                        "warning: could not get project name from path: {}",
                        path.display()
                    );
                    continue;
                }
            }
        };

        println!("Project: {project_path}");
        println!("  Sessions: {sessions:?}");
        println!();
    }

    Ok(())
}
