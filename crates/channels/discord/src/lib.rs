use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use opencrab_core::{Channel, Message};

pub struct DiscordChannel {
    _bot_token: String,
}

impl DiscordChannel {
    pub fn new(bot_token: &str) -> Self {
        Self { _bot_token: bot_token.to_string() }
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    async fn start(&self, _tx: mpsc::UnboundedSender<Message>) -> Result<()> {
        info!("Discord channel started");
        Ok(())
    }

    async fn send_message(&self, _chat_id: &str, _content: &str) -> Result<()> { Ok(()) }
    async fn send_stream_start(&self, _chat_id: &str) -> Result<()> { Ok(()) }
    async fn send_stream_chunk(&self, _chat_id: &str, _chunk: &str) -> Result<()> { Ok(()) }
    async fn send_stream_end(&self, _chat_id: &str) -> Result<()> { Ok(()) }

    fn name(&self) -> &str { "discord" }
    fn supports_streaming(&self) -> bool { false }
}
