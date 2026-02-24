use std::collections::HashMap;
use std::pin::Pin;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

pub mod agent;
pub mod config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub channel: String,
    pub chat_id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

impl Message {
    pub fn new(channel: &str, chat_id: &str, sender: &str, content: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            sender: sender.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatMessage {
    pub fn system(content: &str) -> Self {
        Self {
            role: Role::System,
            content: content.to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: Role::User,
            content: content.to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: content.to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        }
    }

    pub fn tool_result(call_id: &str, content: &str) -> Self {
        Self {
            role: Role::Tool,
            content: content.to_string(),
            tool_call_id: Some(call_id.to_string()),
            tool_calls: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone)]
pub enum StreamChunk {
    Delta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String, name: String, arguments: serde_json::Value, thought_signature: Option<String> },
    Done,
    Error(String),
}

pub type StreamReceiver = mpsc::UnboundedReceiver<StreamChunk>;

pub type StreamItem = Result<String>;
pub type StreamResponse = Pin<Box<dyn futures_core::Stream<Item = StreamItem> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<ProviderResponse>;

    async fn stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<StreamReceiver>;

    fn name(&self) -> &str;
}

#[async_trait]
pub trait Channel: Send + Sync {
    async fn start(&self, tx: mpsc::UnboundedSender<Message>) -> Result<()>;
    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()>;
    async fn send_stream_start(&self, chat_id: &str) -> Result<()>;
    async fn send_stream_chunk(&self, chat_id: &str, chunk: &str) -> Result<()>;
    async fn send_stream_end(&self, chat_id: &str) -> Result<()>;
    fn name(&self) -> &str;
    fn supports_streaming(&self) -> bool;
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDef;
    async fn execute(&self, params: serde_json::Value) -> Result<String>;
    fn name(&self) -> &str;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn save_message(&self, chat_id: &str, msg: &ChatMessage) -> Result<()>;
    async fn get_history(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>>;
    async fn save_fact(&self, user_id: &str, key: &str, value: &str) -> Result<()>;
    async fn get_facts(&self, user_id: &str) -> Result<HashMap<String, String>>;
    async fn clear_history(&self, chat_id: &str) -> Result<()>;
}
