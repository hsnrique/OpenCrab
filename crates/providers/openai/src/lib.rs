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

pub struct OpenAIProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: model.to_string(),
            client: Client::new(),
        }
    }

    fn build_messages(&self, messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages.iter().map(|msg| {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };

            let mut m = json!({ "role": role, "content": msg.content });

            if let Some(id) = &msg.tool_call_id {
                m["tool_call_id"] = json!(id);
            }

            if !msg.tool_calls.is_empty() {
                let calls: Vec<serde_json::Value> = msg.tool_calls.iter().map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": { "name": tc.name, "arguments": tc.arguments.to_string() }
                    })
                }).collect();
                m["tool_calls"] = json!(calls);
            }

            m
        }).collect()
    }

    fn build_tools(&self, tools: &[ToolDef]) -> Vec<serde_json::Value> {
        tools.iter().map(|t| {
            json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } })
        }).collect()
    }

    fn parse_sse_line(line: &str) -> Option<OpenAIStreamChunk> {
        let data = line.strip_prefix("data: ")?;
        if data == "[DONE]" {
            return None;
        }
        serde_json::from_str(data).ok()
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn complete(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<ProviderResponse> {
        let msgs = self.build_messages(messages);
        let mut body = json!({ "model": self.model, "messages": msgs, "temperature": 0.7, "max_tokens": 4096 });
        if !tools.is_empty() { body["tools"] = json!(self.build_tools(tools)); }

        debug!(model = %self.model, "Calling OpenAI API");
        let response = self.client.post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body).send().await.context("Failed to call OpenAI API")?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() { anyhow::bail!("OpenAI API error ({}): {}", status, text); }

        let parsed: OpenAIResponse = serde_json::from_str(&text).context("Failed to parse OpenAI response")?;
        let choice = parsed.choices.first().ok_or_else(|| anyhow::anyhow!("No choices"))?;

        let content = choice.message.content.clone().unwrap_or_default();
        let tool_calls: Vec<ToolCall> = choice.message.tool_calls.as_ref().map(|tcs| {
            tcs.iter().map(|tc| ToolCall {
                id: tc.id.clone(), name: tc.function.name.clone(),
                arguments: serde_json::from_str(&tc.function.arguments).unwrap_or(json!({})),
                thought_signature: None,
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let usage = parsed.usage.map(|u| Usage { input_tokens: u.prompt_tokens, output_tokens: u.completion_tokens });
        info!(model = %self.model, tool_calls = tool_calls.len(), "OpenAI response received");
        Ok(ProviderResponse { content, tool_calls, usage })
    }

    async fn stream(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<StreamReceiver> {
        let msgs = self.build_messages(messages);
        let mut body = json!({ "model": self.model, "messages": msgs, "temperature": 0.7, "max_tokens": 4096, "stream": true });
        if !tools.is_empty() { body["tools"] = json!(self.build_tools(tools)); }

        debug!(model = %self.model, "Calling OpenAI streaming API");
        let response = self.client.post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body).send().await.context("Failed to call OpenAI streaming API")?;

        if !response.status().is_success() {
            let err = response.text().await?;
            anyhow::bail!("OpenAI streaming error: {}", err);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let model = self.model.clone();

        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut tool_args: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new();

            while let Some(result) = byte_stream.next().await {
                let bytes = match result {
                    Ok(b) => b,
                    Err(e) => { let _ = tx.send(StreamChunk::Error(e.to_string())); break; }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() { continue; }
                    if line == "data: [DONE]" {
                        for (_idx, (id, name, args)) in tool_args.drain() {
                            let arguments = serde_json::from_str(&args).unwrap_or(json!({}));
                            let _ = tx.send(StreamChunk::ToolCallEnd { id, name, arguments, thought_signature: None });
                        }
                        break;
                    }

                    if let Some(chunk) = Self::parse_sse_line(&line) {
                        if let Some(choices) = &chunk.choices {
                            for choice in choices {
                                if let Some(delta) = &choice.delta {
                                    if let Some(content) = &delta.content {
                                        let _ = tx.send(StreamChunk::Delta(content.clone()));
                                    }
                                    if let Some(tcs) = &delta.tool_calls {
                                        for tc in tcs {
                                            let idx = tc.index;
                                            let entry = tool_args.entry(idx).or_insert_with(|| {
                                                let id = tc.id.clone().unwrap_or_default();
                                                let name = tc.function.as_ref().map(|f| f.name.clone().unwrap_or_default()).unwrap_or_default();
                                                let _ = tx.send(StreamChunk::ToolCallStart { id: id.clone(), name: name.clone() });
                                                (id, name, String::new())
                                            });
                                            if let Some(f) = &tc.function {
                                                if let Some(args) = &f.arguments {
                                                    entry.2.push_str(args);
                                                    let _ = tx.send(StreamChunk::ToolCallDelta {
                                                        id: entry.0.clone(),
                                                        arguments_delta: args.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let _ = tx.send(StreamChunk::Done);
            info!(model = %model, "OpenAI stream completed");
        });

        Ok(rx)
    }

    fn name(&self) -> &str {
        "openai"
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse { choices: Vec<OpenAIChoice>, usage: Option<OpenAIUsage> }

#[derive(Debug, Deserialize)]
struct OpenAIChoice { message: OpenAIMessage }

#[derive(Debug, Deserialize)]
struct OpenAIMessage { content: Option<String>, tool_calls: Option<Vec<OpenAIToolCall>> }

#[derive(Debug, Deserialize)]
struct OpenAIToolCall { id: String, function: OpenAIFunction }

#[derive(Debug, Deserialize)]
struct OpenAIFunction { name: String, arguments: String }

#[derive(Debug, Deserialize)]
struct OpenAIUsage { prompt_tokens: u32, completion_tokens: u32 }

#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk { choices: Option<Vec<OpenAIStreamChoice>> }

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice { delta: Option<OpenAIStreamDelta> }

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCall {
    index: usize,
    id: Option<String>,
    function: Option<OpenAIStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}
