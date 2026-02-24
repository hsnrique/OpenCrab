use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use opencrab_core::{Channel, Message};

pub struct WhatsAppChannel {
    phone_number_id: String,
    access_token: String,
    verify_token: String,
    webhook_port: u16,
}

impl WhatsAppChannel {
    pub fn new(
        phone_number_id: &str,
        access_token: &str,
        verify_token: &str,
        webhook_port: u16,
    ) -> Self {
        Self {
            phone_number_id: phone_number_id.to_string(),
            access_token: access_token.to_string(),
            verify_token: verify_token.to_string(),
            webhook_port,
        }
    }
}

#[derive(Clone)]
struct AppState {
    tx: mpsc::UnboundedSender<Message>,
    verify_token: String,
}

#[derive(Debug, Deserialize)]
struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebhookPayload {
    entry: Option<Vec<WebhookEntry>>,
}

#[derive(Debug, Deserialize)]
struct WebhookEntry {
    changes: Option<Vec<WebhookChange>>,
}

#[derive(Debug, Deserialize)]
struct WebhookChange {
    value: Option<WebhookValue>,
}

#[derive(Debug, Deserialize)]
struct WebhookValue {
    messages: Option<Vec<WhatsAppMessage>>,
}

#[derive(Debug, Deserialize)]
struct WhatsAppMessage {
    from: String,
    #[serde(rename = "type")]
    msg_type: String,
    text: Option<WhatsAppText>,
}

#[derive(Debug, Deserialize)]
struct WhatsAppText {
    body: String,
}

async fn webhook_verify(
    Query(params): Query<VerifyQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    if params.mode.as_deref() == Some("subscribe")
        && params.token.as_deref() == Some(&state.verify_token)
    {
        info!("WhatsApp webhook verified");
        (StatusCode::OK, params.challenge.unwrap_or_default())
    } else {
        warn!("WhatsApp webhook verification failed");
        (StatusCode::FORBIDDEN, "Forbidden".to_string())
    }
}

async fn webhook_receive(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WebhookPayload>,
) -> impl IntoResponse {
    if let Some(entries) = payload.entry {
        for entry in entries {
            if let Some(changes) = entry.changes {
                for change in changes {
                    if let Some(value) = change.value {
                        if let Some(messages) = value.messages {
                            for msg in messages {
                                if msg.msg_type == "text" {
                                    if let Some(text) = msg.text {
                                        let message = Message::new(
                                            "whatsapp",
                                            &msg.from,
                                            &msg.from,
                                            &text.body,
                                        );
                                        if state.tx.send(message).is_err() {
                                            error!("Failed to forward WhatsApp message");
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
    StatusCode::OK
}

#[async_trait]
impl Channel for WhatsAppChannel {
    async fn start(&self, tx: mpsc::UnboundedSender<Message>) -> Result<()> {
        let state = Arc::new(AppState {
            tx,
            verify_token: self.verify_token.clone(),
        });

        let app = Router::new()
            .route("/webhook", get(webhook_verify))
            .route("/webhook", post(webhook_receive))
            .with_state(state);

        let port = self.webhook_port;
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
                Ok(l) => l,
                Err(e) => {
                    error!(error = %e, "Failed to bind WhatsApp webhook");
                    return;
                }
            };
            info!(port, "WhatsApp webhook server listening");
            if let Err(e) = axum::serve(listener, app).await {
                error!(error = %e, "WhatsApp webhook server error");
            }
        });

        info!(port = self.webhook_port, "WhatsApp channel started");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()> {
        let url = format!(
            "https://graph.facebook.com/v21.0/{}/messages",
            self.phone_number_id
        );
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": chat_id,
            "type": "text",
            "text": { "body": content }
        });

        let resp = reqwest::Client::new()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            error!(error = %err, "WhatsApp API error");
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
        "whatsapp"
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}
