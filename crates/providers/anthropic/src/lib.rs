use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, info};

use opencrab_core::{
    ChatMessage, Provider, ProviderResponse, Role, StreamChunk, StreamReceiver,
    ToolCall, ToolDef, Usage,
};

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self { api_key: api_key.to_string(), model: model.to_string(), client: Client::new() }
    }

    fn extract_system(&self, messages: &[ChatMessage]) -> Option<String> {
        messages.iter().find(|m| matches!(m.role, Role::System)).map(|m| m.content.clone())
    }

    fn build_messages(&self, messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages.iter().filter(|m| !matches!(m.role, Role::System)).map(|msg| {
            if matches!(msg.role, Role::Tool) {
                return json!({
                    "role": "user",
                    "content": [{ "type": "tool_result", "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""), "content": msg.content }]
                });
            }

            let role = match msg.role {
                Role::User | Role::Tool => "user",
                Role::Assistant => "assistant",
                Role::System => unreachable!(),
            };

            if !msg.tool_calls.is_empty() {
                let mut content: Vec<serde_json::Value> = Vec::new();
                if !msg.content.is_empty() {
                    content.push(json!({"type": "text", "text": msg.content}));
                }
                for tc in &msg.tool_calls {
                    content.push(json!({ "type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.arguments }));
                }
                return json!({ "role": role, "content": content });
            }

            json!({ "role": role, "content": msg.content })
        }).collect()
    }

    fn build_tools(&self, tools: &[ToolDef]) -> Vec<serde_json::Value> {
        tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters })).collect()
    }

    fn parse_sse_line(line: &str) -> Option<AnthropicStreamEvent> {
        let data = line.strip_prefix("data: ")?;
        serde_json::from_str(data).ok()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn complete(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<ProviderResponse> {
        let msgs = self.build_messages(messages);
        let mut body = json!({ "model": self.model, "messages": msgs, "max_tokens": 4096 });
        if let Some(system) = self.extract_system(messages) { body["system"] = json!(system); }
        if !tools.is_empty() { body["tools"] = json!(self.build_tools(tools)); }

        debug!(model = %self.model, "Calling Anthropic API");
        let response = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body).send().await.context("Failed to call Anthropic API")?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() { anyhow::bail!("Anthropic API error ({}): {}", status, text); }

        let parsed: AnthropicResponse = serde_json::from_str(&text).context("Failed to parse Anthropic response")?;
        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for block in &parsed.content {
            match block.block_type.as_str() {
                "text" => { if let Some(t) = &block.text { content.push_str(t); } }
                "tool_use" => {
                    if let (Some(id), Some(name), Some(input)) = (&block.id, &block.name, &block.input) {
                        tool_calls.push(ToolCall { id: id.clone(), name: name.clone(), arguments: input.clone(), thought_signature: None });
                    }
                }
                _ => {}
            }
        }

        let usage = Some(Usage { input_tokens: parsed.usage.input_tokens, output_tokens: parsed.usage.output_tokens });
        info!(model = %self.model, tool_calls = tool_calls.len(), "Anthropic response received");
        Ok(ProviderResponse { content, tool_calls, usage })
    }

    async fn stream(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<StreamReceiver> {
        let msgs = self.build_messages(messages);
        let mut body = json!({ "model": self.model, "messages": msgs, "max_tokens": 4096, "stream": true });
        if let Some(system) = self.extract_system(messages) { body["system"] = json!(system); }
        if !tools.is_empty() { body["tools"] = json!(self.build_tools(tools)); }

        debug!(model = %self.model, "Calling Anthropic streaming API");
        let response = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body).send().await.context("Failed to call Anthropic streaming API")?;

        if !response.status().is_success() {
            let err = response.text().await?;
            anyhow::bail!("Anthropic streaming error: {}", err);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let model = self.model.clone();

        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut current_tool_id = String::new();
            let mut current_tool_name = String::new();
            let mut current_tool_args = String::new();

            while let Some(result) = byte_stream.next().await {
                let bytes = match result {
                    Ok(b) => b,
                    Err(e) => { let _ = tx.send(StreamChunk::Error(e.to_string())); break; }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() || line.starts_with("event:") {
                        continue;
                    }

                    if let Some(event) = Self::parse_sse_line(&line) {
                        match event.event_type.as_deref().unwrap_or("") {
                            _ => {
                                if let Some(delta) = &event.delta {
                                    if let Some(text) = &delta.text {
                                        let _ = tx.send(StreamChunk::Delta(text.clone()));
                                    }
                                    if let Some(args) = &delta.partial_json {
                                        current_tool_args.push_str(args);
                                        let _ = tx.send(StreamChunk::ToolCallDelta {
                                            id: current_tool_id.clone(),
                                            arguments_delta: args.clone(),
                                        });
                                    }
                                }

                                if let Some(cb) = &event.content_block {
                                    if cb.block_type.as_deref() == Some("tool_use") {
                                        current_tool_id = cb.id.clone().unwrap_or_default();
                                        current_tool_name = cb.name.clone().unwrap_or_default();
                                        current_tool_args.clear();
                                        let _ = tx.send(StreamChunk::ToolCallStart {
                                            id: current_tool_id.clone(),
                                            name: current_tool_name.clone(),
                                        });
                                    }
                                }

                                if event.event_type.as_deref() == Some("content_block_stop") && !current_tool_id.is_empty() {
                                    let arguments = serde_json::from_str(&current_tool_args).unwrap_or(json!({}));
                                    let _ = tx.send(StreamChunk::ToolCallEnd {
                                        id: current_tool_id.clone(),
                                        name: current_tool_name.clone(),
                                        arguments,
                                        thought_signature: None,
                                    });
                                    current_tool_id.clear();
                                    current_tool_name.clear();
                                    current_tool_args.clear();
                                }
                            }
                        }
                    }
                }
            }

            let _ = tx.send(StreamChunk::Done);
            info!(model = %model, "Anthropic stream completed");
        });

        Ok(rx)
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse { content: Vec<AnthropicBlock>, usage: AnthropicUsage }

#[derive(Debug, Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")] block_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage { input_tokens: u32, output_tokens: u32 }

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    delta: Option<AnthropicDelta>,
    content_block: Option<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
}
