use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{
    Context, CreateMessage, EventHandler, GatewayIntents, Message as SerenityMessage, Ready,
};
use tokio::sync::mpsc;
use tracing::{error, info};

use opencrab_core::{Channel, Message};

pub struct DiscordChannel {
    bot_token: String,
}

impl DiscordChannel {
    pub fn new(bot_token: &str) -> Self {
        Self {
            bot_token: bot_token.to_string(),
        }
    }
}

struct Handler {
    tx: mpsc::UnboundedSender<Message>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: SerenityMessage) {
        if msg.author.bot {
            return;
        }

        let content = msg.content.trim().to_string();
        if content.is_empty() {
            return;
        }

        let message = Message::new(
            "discord",
            &msg.channel_id.to_string(),
            &msg.author.name,
            &content,
        );

        if self.tx.send(message).is_err() {
            error!("Failed to forward Discord message");
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "Discord bot connected");
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    async fn start(&self, tx: mpsc::UnboundedSender<Message>) -> Result<()> {
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = Handler { tx };

        let token = self.bot_token.clone();
        tokio::spawn(async move {
            let mut client = match serenity::Client::builder(&token, intents)
                .event_handler(handler)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "Failed to create Discord client");
                    return;
                }
            };

            if let Err(e) = client.start().await {
                error!(error = %e, "Discord client error");
            }
        });

        info!("Discord channel started");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()> {
        let channel_id: u64 = chat_id.parse().unwrap_or(0);
        if channel_id == 0 {
            return Ok(());
        }

        let http = serenity::http::Http::new(&self.bot_token);
        let channel = serenity::model::id::ChannelId::new(channel_id);

        let chunks = split_message(content, 2000);
        for chunk in chunks {
            let msg = CreateMessage::new().content(&chunk);
            if let Err(e) = channel.send_message(&http, msg).await {
                error!(error = %e, "Failed to send Discord message");
            }
        }

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
        "discord"
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}

fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if current.len() + line.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current.clone());
                current.clear();
            }
            if line.len() > max_len {
                for i in (0..line.len()).step_by(max_len) {
                    let end = (i + max_len).min(line.len());
                    chunks.push(line[i..end].to_string());
                }
                continue;
            }
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
