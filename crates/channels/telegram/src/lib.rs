use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use opencrab_core::{Channel, Message as CrabMessage};

pub struct TelegramChannel {
    bot_token: String,
}

impl TelegramChannel {
    pub fn new(bot_token: &str) -> Self {
        Self { bot_token: bot_token.to_string() }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    async fn start(&self, tx: mpsc::UnboundedSender<CrabMessage>) -> Result<()> {
        info!("Telegram channel started");
        let bot = teloxide::Bot::new(&self.bot_token);
        use teloxide::prelude::*;

        let tx_clone = tx.clone();
        let handler = Update::filter_message().endpoint(
            move |msg: teloxide::types::Message, _bot: teloxide::Bot| {
                let tx = tx_clone.clone();
                async move {
                    if let Some(text) = msg.text() {
                        let chat_id = msg.chat.id.0.to_string();
                        let sender = msg.from.as_ref()
                            .map(|u| u.username.clone().unwrap_or_else(|| u.first_name.clone()))
                            .unwrap_or_else(|| "unknown".to_string());
                        let _ = tx.send(CrabMessage::new("telegram", &chat_id, &sender, text));
                    }
                    respond(())
                }
            },
        );

        teloxide::dispatching::Dispatcher::builder(bot.clone(), handler).build().dispatch().await;
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()> {
        let bot = teloxide::Bot::new(&self.bot_token);
        let chat_id: i64 = chat_id.parse()?;
        use teloxide::prelude::*;
        use teloxide::types::ChatId;
        bot.send_message(ChatId(chat_id), content).await?;
        Ok(())
    }

    async fn send_stream_start(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn send_stream_chunk(&self, _chat_id: &str, _chunk: &str) -> Result<()> {
        Ok(())
    }

    async fn send_stream_end(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "telegram"
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}
