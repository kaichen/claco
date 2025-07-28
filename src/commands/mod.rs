pub mod agents;
pub mod history;
pub mod hooks;
pub mod projects;
pub mod session;
pub mod settings;
pub mod slash_commands;

pub use agents::handle_agents;
pub use history::handle_history;
pub use hooks::handle_hooks;
pub use projects::handle_projects;
pub use session::handle_session;
pub use settings::handle_settings;
pub use slash_commands::handle_commands;

use chrono::{DateTime, Local};

/// Format timestamp from UTC to local timezone
pub fn format_timestamp_local(timestamp_str: &str) -> String {
    // Try to parse the timestamp as UTC and convert to local timezone
    match DateTime::parse_from_rfc3339(timestamp_str) {
        Ok(dt) => {
            let local_dt: DateTime<Local> = dt.with_timezone(&Local);
            local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        Err(_) => timestamp_str.to_string(), // If parsing fails, return original
    }
}
