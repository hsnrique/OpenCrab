use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;
use tracing::info;

use opencrab_core::{Tool, ToolDef};

pub struct SystemInfoTool;

impl SystemInfoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "system_info".to_string(),
            description: "Get system information: OS details, CPU, memory, disk usage, running processes, network, or environment variables.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "What info to retrieve",
                        "enum": ["overview", "processes", "disk", "network", "env"]
                    }
                },
                "required": ["category"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let category = params["category"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'category' parameter"))?;

        info!(category, "Getting system info");

        let script = match category {
            "overview" => {
                r#"echo "=== System ===" && uname -a && echo "" && echo "=== Uptime ===" && uptime && echo "" && echo "=== Memory ===" && vm_stat 2>/dev/null || free -h 2>/dev/null && echo "" && echo "=== CPU ===" && sysctl -n machdep.cpu.brand_string 2>/dev/null || cat /proc/cpuinfo 2>/dev/null | head -5"#
            }
            "processes" => {
                "ps aux --sort=-%mem 2>/dev/null | head -15 || ps aux | head -15"
            }
            "disk" => "df -h",
            "network" => {
                r#"echo "=== Interfaces ===" && ifconfig 2>/dev/null | grep -E 'inet |flags' || ip addr 2>/dev/null && echo "" && echo "=== Connections ===" && netstat -an 2>/dev/null | head -20 || ss -tuln 2>/dev/null | head -20"#
            }
            "env" => {
                "env | sort | head -30"
            }
            _ => return Ok(format!("Unknown category: {category}")),
        };

        let output = Command::new("sh")
            .arg("-c")
            .arg(script)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.is_empty() {
            Ok("(no output)".to_string())
        } else if stdout.len() > 6000 {
            Ok(format!("{}... (truncated)", &stdout[..6000]))
        } else {
            Ok(stdout.to_string())
        }
    }
}
