use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;
use tracing::{info, warn};

use opencrab_core::{Tool, ToolDef};

pub struct ShellTool {
    allowed_commands: Vec<String>,
}

impl ShellTool {
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self { allowed_commands }
    }

    fn is_allowed(&self, command: &str) -> bool {
        if self.allowed_commands.is_empty() {
            return true;
        }

        let first_word = command.split_whitespace().next().unwrap_or("");
        self.allowed_commands.iter().any(|c| c == first_word)
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "shell".to_string(),
            description: "Execute a shell command on the user's system and return the output."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let command = params["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;

        if !self.is_allowed(command) {
            warn!(command, "Blocked disallowed command");
            return Ok(format!("Command not allowed: {command}"));
        }

        info!(command, "Executing shell command");

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if output.status.success() {
            if stdout.is_empty() {
                "(command completed successfully with no output)".to_string()
            } else {
                truncate_output(&stdout, 4000)
            }
        } else {
            format!(
                "Command failed (exit code: {}):\n{}{}",
                output.status.code().unwrap_or(-1),
                truncate_output(&stdout, 2000),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!("\nStderr:\n{}", truncate_output(&stderr, 2000))
                }
            )
        };

        Ok(result)
    }
}

fn truncate_output(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!(
            "{}...\n(output truncated, showing first {} chars of {})",
            &text[..max_chars],
            max_chars,
            text.len()
        )
    }
}
