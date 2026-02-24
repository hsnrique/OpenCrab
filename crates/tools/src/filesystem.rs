use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tracing::info;

use opencrab_core::{Tool, ToolDef};

pub struct FileSystemTool {
    root: PathBuf,
}

impl FileSystemTool {
    pub fn new(root: Option<String>) -> Self {
        let root = root
            .map(|r| {
                if let Some(stripped) = r.strip_prefix("~/") {
                    let home = std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("/tmp"));
                    home.join(stripped)
                } else {
                    PathBuf::from(r)
                }
            })
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
            });

        Self { root }
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let resolved = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        };

        let canonical = resolved.canonicalize().unwrap_or(resolved.clone());

        if !canonical.starts_with(&self.root) {
            anyhow::bail!(
                "Access denied: path '{}' is outside the allowed root '{}'",
                path,
                self.root.display()
            );
        }

        Ok(resolved)
    }
}

#[async_trait]
impl Tool for FileSystemTool {
    fn name(&self) -> &str {
        "filesystem"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "filesystem".to_string(),
            description: "Read, write, or list files and directories on the user's system."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write", "list", "exists"],
                        "description": "The filesystem action to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "The file or directory path"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write (only for 'write' action)"
                    }
                },
                "required": ["action", "path"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let action = params["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let path = self.resolve_path(path_str)?;

        match action {
            "read" => {
                info!(path = %path.display(), "Reading file");
                let content = tokio::fs::read_to_string(&path).await?;
                Ok(content)
            }
            "write" => {
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter for write"))?;

                info!(path = %path.display(), "Writing file");

                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&path, content).await?;
                Ok(format!("Successfully wrote to {}", path.display()))
            }
            "list" => {
                info!(path = %path.display(), "Listing directory");
                let mut entries = tokio::fs::read_dir(&path).await?;
                let mut listing = Vec::new();

                while let Some(entry) = entries.next_entry().await? {
                    let file_type = entry.file_type().await?;
                    let prefix = if file_type.is_dir() { "d" } else { "f" };
                    let name = entry.file_name().to_string_lossy().to_string();
                    listing.push(format!("[{prefix}] {name}"));
                }

                listing.sort();

                if listing.is_empty() {
                    Ok("(empty directory)".to_string())
                } else {
                    Ok(listing.join("\n"))
                }
            }
            "exists" => {
                let exists = path.exists();
                Ok(format!("{}: {}", path.display(), if exists { "exists" } else { "not found" }))
            }
            _ => Ok(format!("Unknown action: {action}")),
        }
    }
}
