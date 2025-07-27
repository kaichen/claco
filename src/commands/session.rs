use anyhow::Result;
use claco::{claude_home, SessionEntry};
use std::fs;
use std::io::{BufRead, BufReader};

use super::format_timestamp_local;

pub fn handle_session(session_id: Option<String>) -> Result<()> {
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
