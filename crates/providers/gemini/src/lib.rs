use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, info, error};

use opencrab_core::{
    ChatMessage, Provider, ProviderResponse, Role, StreamChunk, StreamReceiver,
    ToolCall, ToolDef, Usage,
};

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: Client,
    search_enabled: bool,
}

fn sanitize_tool_output(output: &str) -> String {
    let stripped: String = output
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
        .collect();

    let max_len = 16_000;
    if stripped.len() > max_len {
        format!("{}... [truncated, {} total chars]", &stripped[..max_len], stripped.len())
    } else {
        stripped
    }
}

impl GeminiProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: model.to_string(),
            client: Client::new(),
            search_enabled: false,
        }
    }

    pub fn with_search(mut self, enabled: bool) -> Self {
        self.search_enabled = enabled;
        self
    }

    fn build_request_body(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> serde_json::Value {
        let contents = self.build_contents(messages);
        let mut body = json!({
            "contents": contents,
            "generationConfig": { "temperature": 0.7, "maxOutputTokens": 8192 }
        });

        if let Some(system) = self.extract_system_instruction(messages) {
            body["systemInstruction"] = json!({ "parts": [{"text": system}] });
        }

        if let Some(tool_config) = self.build_tools(tools) {
            body["tools"] = tool_config;
        }

        body
    }

    fn build_contents(&self, messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        let mut contents = Vec::new();

        for msg in messages {
            if matches!(msg.role, Role::System) {
                continue;
            }

            if matches!(msg.role, Role::Tool) {
                if let Some(call_id) = &msg.tool_call_id {
                    let sanitized = sanitize_tool_output(&msg.content);
                    contents.push(json!({
                        "role": "function",
                        "parts": [{ "functionResponse": { "name": call_id, "response": { "name": call_id, "content": sanitized } } }]
                    }));
                }
                continue;
            }

            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "model",
                _ => continue,
            };

            let mut parts = Vec::new();
            if !msg.content.is_empty() {
                parts.push(json!({"text": msg.content}));
            }
            for tc in &msg.tool_calls {
                let mut fc_part = json!({ "functionCall": { "name": tc.name, "args": tc.arguments } });
                if let Some(sig) = &tc.thought_signature {
                    fc_part["thoughtSignature"] = json!(sig);
                }
                parts.push(fc_part);
            }
            if !parts.is_empty() {
                contents.push(json!({ "role": role, "parts": parts }));
            }
        }

        contents
    }

    fn build_tools(&self, tools: &[ToolDef]) -> Option<serde_json::Value> {
        let mut tool_entries = Vec::new();

        if !tools.is_empty() {
            let decls: Vec<_> = tools.iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "parameters": t.parameters }))
                .collect();
            tool_entries.push(json!({ "functionDeclarations": decls }));
        }

        if self.search_enabled {
            tool_entries.push(json!({ "google_search": {} }));
        }

        if tool_entries.is_empty() {
            None
        } else {
            Some(json!(tool_entries))
        }
    }

    fn extract_system_instruction(&self, messages: &[ChatMessage]) -> Option<String> {
        messages.iter().find(|m| matches!(m.role, Role::System)).map(|m| m.content.clone())
    }

    fn parse_sse_line(line: &str) -> Option<GeminiStreamChunk> {
        let data = line.strip_prefix("data: ")?;
        if data == "[DONE]" {
            return None;
        }
        serde_json::from_str(data).ok()
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    async fn complete(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<ProviderResponse> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
        let body = self.build_request_body(messages, tools);

        debug!(model = %self.model, "Calling Gemini API");
        let response = self.client.post(&url).json(&body).send().await.context("Failed to call Gemini API")?;
        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            error!(status = %status, body = %text, "Gemini API error");
            anyhow::bail!("Gemini API error ({}): {}", status, text);
        }

        let parsed: GeminiResponse = serde_json::from_str(&text).context("Failed to parse Gemini response")?;
        let candidate = parsed.candidates.first().ok_or_else(|| anyhow::anyhow!("No candidates"))?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        if let Some(candidate_content) = &candidate.content {
            for part in &candidate_content.parts {
                if let Some(t) = &part.text { content.push_str(t); }
                if let Some(fc) = &part.function_call {
                    tool_calls.push(ToolCall {
                        id: fc.name.clone(),
                        name: fc.name.clone(),
                        arguments: fc.args.clone(),
                        thought_signature: part.thought_signature.clone(),
                    });
                }
            }
        }

        let usage = parsed.usage_metadata.map(|u| Usage { input_tokens: u.prompt_token_count, output_tokens: u.candidates_token_count });
        info!(model = %self.model, tool_calls = tool_calls.len(), "Gemini response received");
        Ok(ProviderResponse { content, tool_calls, usage })
    }

    async fn stream(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> Result<StreamReceiver> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        );
        let body = self.build_request_body(messages, tools);

        debug!(model = %self.model, "Calling Gemini streaming API");
        let response = self.client.post(&url).json(&body).send().await.context("Failed to call Gemini streaming API")?;

        let status = response.status();
        if !status.is_success() {
            let err = response.text().await?;
            anyhow::bail!("Gemini streaming error ({}): {}", status, err);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let model = self.model.clone();

        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(result) = byte_stream.next().await {
                let bytes = match result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(StreamChunk::Error(e.to_string()));
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    if let Some(chunk) = Self::parse_sse_line(&line) {
                        if let Some(candidates) = &chunk.candidates {
                            for candidate in candidates {
                                if let Some(content) = &candidate.content {
                                    for part in &content.parts {
                                        if let Some(text) = &part.text {
                                            let _ = tx.send(StreamChunk::Delta(text.clone()));
                                        }
                                        if let Some(fc) = &part.function_call {
                                            let _ = tx.send(StreamChunk::ToolCallEnd {
                                                id: fc.name.clone(),
                                                name: fc.name.clone(),
                                                arguments: fc.args.clone(),
                                                thought_signature: part.thought_signature.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let _ = tx.send(StreamChunk::Done);
            info!(model = %model, "Gemini stream completed");
        });

        Ok(rx)
    }

    fn name(&self) -> &str {
        "gemini"
    }
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Debug, Deserialize)]
struct GeminiStreamChunk {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
}
