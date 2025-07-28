use anyhow::{bail, Context, Result};
use std::process::{Command, Output, Stdio};

/// Output from Claude CLI execution
#[derive(Debug, Clone)]
pub struct ClaudeOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl ClaudeOutput {
    /// Get stdout as lines
    pub fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }

    /// Check if output is empty
    pub fn is_empty(&self) -> bool {
        self.stdout.trim().is_empty()
    }
}

/// Builder for Claude CLI commands
#[derive(Debug, Clone, Default)]
pub struct ClaudeCli {
    print_mode: bool,
    system_prompt: Option<String>,
    model: Option<String>,
    output_format: Option<String>,
    additional_args: Vec<String>,
}

impl ClaudeCli {
    /// Create a new Claude CLI builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable print mode (non-interactive)
    pub fn print_mode(mut self) -> Self {
        self.print_mode = true;
        self
    }

    /// Set system prompt to append
    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string());
        self
    }

    /// Set model to use
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set output format (text, json, stream-json)
    pub fn with_output_format(mut self, format: &str) -> Self {
        self.output_format = Some(format.to_string());
        self
    }

    /// Add additional arguments
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.additional_args = args;
        self
    }

    /// Execute claude command with the given prompt
    pub fn execute(&self, prompt: &str) -> Result<ClaudeOutput> {
        let mut cmd = Command::new("claude");

        // Add print mode flag
        if self.print_mode {
            cmd.arg("--print");
        }

        // Add system prompt if provided
        if let Some(ref system_prompt) = self.system_prompt {
            cmd.arg("--append-system-prompt");
            cmd.arg(system_prompt);
        }

        // Add model if provided
        if let Some(ref model) = self.model {
            cmd.arg("--model");
            cmd.arg(model);
        }

        // Add output format if provided
        if let Some(ref format) = self.output_format {
            cmd.arg("--output-format");
            cmd.arg(format);
        }

        // Add any additional arguments
        for arg in &self.additional_args {
            cmd.arg(arg);
        }

        // Add the prompt
        cmd.arg(prompt);

        // Configure stdio
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Execute command
        let output = cmd.output().context("Failed to execute claude command")?;

        Ok(self.parse_output(output))
    }

    /// Parse command output into ClaudeOutput
    fn parse_output(&self, output: Output) -> ClaudeOutput {
        ClaudeOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
        }
    }
}

/// Simple helper to ask Claude a question in print mode
pub fn ask_claude(prompt: &str) -> Result<String> {
    let output = ClaudeCli::new().print_mode().execute(prompt)?;

    if !output.success {
        bail!("Claude command failed: {}", output.stderr);
    }

    Ok(output.stdout)
}

/// Generate an agent with Claude
pub fn generate_agent(prompt: &str) -> Result<(String, String)> {
    let system_prompt = r#"You are an agent generator for Claude Code. Generate a custom agent based on the user's request.

IMPORTANT: Your response MUST start with the line:
filename: <agent-name>.md

Where <agent-name> is a descriptive, kebab-case name for the agent.

Then provide the complete agent markdown content following this structure:
---
agentType: <type>
tools: [<tool1>, <tool2>, ...]
---

# Agent Name

Description of what the agent does.

## Prompt

The actual prompt for the agent.

Make sure the agent is practical, well-defined, and follows Claude Code agent conventions."#;

    let claude_prompt = format!("Generate a custom agent markdown for: {prompt}");

    let output = ClaudeCli::new()
        .print_mode()
        .with_system_prompt(system_prompt)
        .execute(&claude_prompt)?;

    if !output.success {
        bail!("Failed to generate agent: {}", output.stderr);
    }

    parse_filename_content(&output.stdout)
}

/// Generate a slash command with Claude
pub fn generate_command(prompt: &str) -> Result<(String, String)> {
    let system_prompt = r#"You are a slash command generator for Claude Code. Generate a custom slash command based on the user's request.

IMPORTANT: Your response MUST start with the line:
filename: <command-name>.md

Where <command-name> is a descriptive, kebab-case name for the command (without the leading slash).

Then provide the complete slash command markdown content.

The command should be practical, well-defined, and follow Claude Code slash command conventions.
Focus on making the command reusable and clear in its purpose."#;

    let claude_prompt = format!("Generate a slash command markdown for: {prompt}");

    let output = ClaudeCli::new()
        .print_mode()
        .with_system_prompt(system_prompt)
        .execute(&claude_prompt)?;

    if !output.success {
        bail!("Failed to generate command: {}", output.stderr);
    }

    parse_filename_content(&output.stdout)
}

/// Parse output that starts with "filename: " line
fn parse_filename_content(output: &str) -> Result<(String, String)> {
    let lines: Vec<&str> = output.lines().collect();

    if lines.is_empty() {
        bail!("No output from Claude");
    }

    // Extract filename from first line
    let first_line = lines[0];
    if !first_line.starts_with("filename:") {
        bail!("Invalid output format. Expected 'filename:' on first line");
    }

    let filename = first_line
        .trim_start_matches("filename:")
        .trim()
        .to_string();

    // Rest is content
    let content = if lines.len() > 1 {
        lines[1..].join("\n")
    } else {
        String::new()
    };

    Ok((filename, content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_cli_builder() {
        let cli = ClaudeCli::new()
            .print_mode()
            .with_system_prompt("Test system")
            .with_model("claude-3-opus");

        assert!(cli.print_mode);
        assert_eq!(cli.system_prompt, Some("Test system".to_string()));
        assert_eq!(cli.model, Some("claude-3-opus".to_string()));
    }

    #[test]
    fn test_parse_filename_content() {
        let output = "filename: test-agent.md\n# Test Agent\n\nThis is a test";
        let (filename, content) = parse_filename_content(output).unwrap();

        assert_eq!(filename, "test-agent.md");
        assert_eq!(content, "# Test Agent\n\nThis is a test");
    }

    #[test]
    fn test_parse_filename_content_no_filename() {
        let output = "This is just content";
        let result = parse_filename_content(output);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Expected 'filename:'"));
    }

    #[test]
    fn test_claude_output_helpers() {
        let output = ClaudeOutput {
            stdout: "line1\nline2\nline3".to_string(),
            stderr: String::new(),
            success: true,
        };

        assert_eq!(output.lines(), vec!["line1", "line2", "line3"]);
        assert!(!output.is_empty());

        let empty_output = ClaudeOutput {
            stdout: "  \n  ".to_string(),
            stderr: String::new(),
            success: true,
        };

        assert!(empty_output.is_empty());
    }
}
