use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::info;

use opencrab_core::{Tool, ToolDef};

pub struct UrlReaderTool {
    client: Client,
}

impl UrlReaderTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

#[async_trait]
impl Tool for UrlReaderTool {
    fn name(&self) -> &str {
        "url_reader"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "url_reader".to_string(),
            description: "Fetch a URL and extract readable text content from the page. Useful for reading articles, documentation, and web pages.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch and read"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        info!(url, "Fetching URL");

        let response = self.client
            .get(url)
            .header("User-Agent", "OpenCrab/1.0 (AI Assistant)")
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => return Ok(format!("Failed to fetch URL: {e}")),
        };

        let status = response.status();
        if !status.is_success() {
            return Ok(format!("HTTP error: {status}"));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return Ok(format!("Failed to read response body: {e}")),
        };

        let text = if content_type.contains("text/html") {
            extract_text_from_html(&body)
        } else {
            body
        };

        let max_len = 8000;
        if text.len() > max_len {
            Ok(format!(
                "{}...\n\n(content truncated, showing {} of {} chars)",
                &text[..max_len],
                max_len,
                text.len()
            ))
        } else {
            Ok(text)
        }
    }
}

fn extract_text_from_html(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut collecting_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                collecting_tag = true;
                tag_name.clear();
            }
            '>' => {
                in_tag = false;
                collecting_tag = false;
                let tag_lower = tag_name.to_lowercase();
                if tag_lower.starts_with("script") {
                    in_script = true;
                } else if tag_lower.starts_with("/script") {
                    in_script = false;
                } else if tag_lower.starts_with("style") {
                    in_style = true;
                } else if tag_lower.starts_with("/style") {
                    in_style = false;
                } else if matches!(tag_lower.as_str(), "br" | "br/" | "p" | "/p" | "div" | "/div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "/h1" | "/h2" | "/h3" | "/h4" | "/h5" | "/h6" | "li" | "/li" | "tr" | "/tr") {
                    result.push('\n');
                }
            }
            _ if in_tag => {
                if collecting_tag && ch != ' ' && ch != '/' {
                    tag_name.push(ch);
                } else {
                    collecting_tag = false;
                }
            }
            _ if !in_script && !in_style => {
                result.push(ch);
            }
            _ => {}
        }
    }

    let decoded = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    let lines: Vec<&str> = decoded
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    lines.join("\n")
}
