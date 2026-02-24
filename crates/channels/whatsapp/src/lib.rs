use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use opencrab_core::{Channel, Message};

pub struct WhatsAppChannel {
    phone_number_id: String,
    access_token: String,
    _verify_token: String,
    webhook_port: u16,
}

impl WhatsAppChannel {
    pub fn new(phone_number_id: &str, access_token: &str, verify_token: &str, webhook_port: u16) -> Self {
        Self {
            phone_number_id: phone_number_id.to_string(),
            access_token: access_token.to_string(),
            _verify_token: verify_token.to_string(),
            webhook_port,
        }
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    async fn start(&self, _tx: mpsc::UnboundedSender<Message>) -> Result<()> {
        info!(port = self.webhook_port, "WhatsApp channel started (webhook mode)");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()> {
        let url = format!("https://graph.facebook.com/v21.0/{}/messages", self.phone_number_id);
        let body = serde_json::json!({
            "messaging_product": "whatsapp", "to": chat_id,
            "type": "text", "text": { "body": content }
        });
        reqwest::Client::new().post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&body).send().await?;
        Ok(())
    }

    async fn send_stream_start(&self, _chat_id: &str) -> Result<()> { Ok(()) }
    async fn send_stream_chunk(&self, _chat_id: &str, _chunk: &str) -> Result<()> { Ok(()) }
    async fn send_stream_end(&self, _chat_id: &str) -> Result<()> { Ok(()) }

    fn name(&self) -> &str { "whatsapp" }
    fn supports_streaming(&self) -> bool { false }
}
