use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;
use tracing::info;

use opencrab_core::{Tool, ToolDef};

#[derive(Default)]
pub struct CodeRunnerTool;

impl CodeRunnerTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CodeRunnerTool {
    fn name(&self) -> &str {
        "code_runner"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "code_runner".to_string(),
            description: "Execute code snippets in Python, Node.js, or Bash and return the output. Use this for calculations, data processing, or testing code.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "language": {
                        "type": "string",
                        "description": "Programming language: python, node, or bash",
                        "enum": ["python", "node", "bash"]
                    },
                    "code": {
                        "type": "string",
                        "description": "The code to execute"
                    }
                },
                "required": ["language", "code"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let language = params["language"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'language' parameter"))?;

        let code = params["code"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'code' parameter"))?;

        info!(language, "Running code snippet");

        let (cmd, args) = match language {
            "python" => ("python3", vec!["-c", code]),
            "node" => ("node", vec!["-e", code]),
            "bash" => ("bash", vec!["-c", code]),
            _ => return Ok(format!("Unsupported language: {language}")),
        };

        let output = Command::new(cmd)
            .args(&args)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);

                if out.status.success() {
                    let result = if stdout.is_empty() {
                        "(executed successfully, no output)".to_string()
                    } else {
                        truncate(&stdout, 8000)
                    };
                    Ok(result)
                } else {
                    Ok(format!(
                        "Exit code: {}\n{}{}",
                        out.status.code().unwrap_or(-1),
                        truncate(&stdout, 4000),
                        if stderr.is_empty() {
                            String::new()
                        } else {
                            format!("\nStderr:\n{}", truncate(&stderr, 4000))
                        }
                    ))
                }
            }
            Err(e) => Ok(format!(
                "Failed to run {language}: {e}. Make sure {cmd} is installed."
            )),
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}... (truncated)", &text[..max])
    }
}
