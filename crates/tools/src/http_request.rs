use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::info;

use opencrab_core::{Tool, ToolDef};

pub struct HttpRequestTool {
    client: Client,
}

impl HttpRequestTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "http_request".to_string(),
            description: "Make an HTTP request to any URL. Supports GET, POST, PUT, DELETE with custom headers and JSON body. Useful for API calls.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "method": {
                        "type": "string",
                        "description": "HTTP method: GET, POST, PUT, DELETE",
                        "enum": ["GET", "POST", "PUT", "DELETE"]
                    },
                    "url": {
                        "type": "string",
                        "description": "The URL to request"
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional HTTP headers as key-value pairs"
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional request body (JSON string for POST/PUT)"
                    }
                },
                "required": ["method", "url"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let method = params["method"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?
            .to_uppercase();

        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        info!(method = %method, url, "Making HTTP request");

        let mut request = match method.as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => return Ok(format!("Unsupported HTTP method: {method}")),
        };

        request = request.header("User-Agent", "OpenCrab/1.0");

        if let Some(headers) = params["headers"].as_object() {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request = request.header(key.as_str(), v);
                }
            }
        }

        if let Some(body) = params["body"].as_str() {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return Ok(format!("Request failed: {e}")),
        };

        let status = response.status();
        let headers: Vec<String> = response
            .headers()
            .iter()
            .take(10)
            .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or("(binary)")))
            .collect();

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return Ok(format!("Failed to read response: {e}")),
        };

        let max_len = 8000;
        let body_display = if body.len() > max_len {
            format!("{}...\n(truncated, {} total bytes)", &body[..max_len], body.len())
        } else {
            body
        };

        Ok(format!(
            "Status: {status}\nHeaders:\n{}\n\nBody:\n{body_display}",
            headers.join("\n")
        ))
    }
}
