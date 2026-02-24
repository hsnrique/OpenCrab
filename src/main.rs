use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use opencrab_core::agent::Agent;
use opencrab_core::config::Config;
use opencrab_core::{Channel, Provider};

use opencrab_memory::SqliteMemory;
use opencrab_tools::{FileSystemTool, ShellTool};

fn create_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    let provider_name = &config.agent.default_provider;

    match provider_name.as_str() {
        "gemini" => {
            let entry = config.providers.gemini.as_ref().context("Gemini provider not configured")?;
            Ok(Arc::new(opencrab_provider_gemini::GeminiProvider::new(&entry.api_key, &entry.model)))
        }
        "openai" => {
            let entry = config.providers.openai.as_ref().context("OpenAI provider not configured")?;
            Ok(Arc::new(opencrab_provider_openai::OpenAIProvider::new(&entry.api_key, &entry.model)))
        }
        "anthropic" => {
            let entry = config.providers.anthropic.as_ref().context("Anthropic provider not configured")?;
            Ok(Arc::new(opencrab_provider_anthropic::AnthropicProvider::new(&entry.api_key, &entry.model)))
        }
        _ => anyhow::bail!("Unknown provider: {provider_name}"),
    }
}

fn create_channels(config: &Config) -> Vec<Arc<dyn Channel>> {
    let mut channels: Vec<Arc<dyn Channel>> = Vec::new();

    if let Some(cli) = &config.channels.cli {
        if cli.enabled {
            channels.push(Arc::new(opencrab_channel_cli::CliChannel::new()));
        }
    }

    if let Some(tg) = &config.channels.telegram {
        if tg.enabled {
            channels.push(Arc::new(opencrab_channel_telegram::TelegramChannel::new(&tg.bot_token)));
        }
    }

    channels
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = if std::path::Path::new("config.toml").exists() {
        Config::load("config.toml")?
    } else {
        info!("No config.toml found, using defaults (CLI mode, no provider)");
        Config::default_config()
    };

    info!(name = %config.agent.name, provider = %config.agent.default_provider, "Starting OpenCrab 🦀");

    let provider = create_provider(&config)?;
    let memory = Arc::new(SqliteMemory::new(&config.memory.database_path)?);

    let mut agent = Agent::new(config.clone(), provider, memory);

    if config.tools.shell_enabled {
        agent.register_tool(Arc::new(ShellTool::new(config.tools.shell_allowed_commands.clone())));
    }

    if config.tools.filesystem_enabled {
        agent.register_tool(Arc::new(FileSystemTool::new(config.tools.filesystem_root.clone())));
    }

    let agent = Arc::new(agent);
    let channels = create_channels(&config);

    if channels.is_empty() {
        anyhow::bail!("No channels enabled. Enable at least one channel in config.toml.");
    }

    let (tx, mut rx) = mpsc::unbounded_channel();

    for channel in &channels {
        let ch = channel.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = ch.start(tx).await {
                error!(channel = %ch.name(), error = %e, "Channel failed to start");
            }
        });
    }

    drop(tx);

    let channels_map: std::collections::HashMap<String, Arc<dyn Channel>> = channels
        .into_iter()
        .map(|c| (c.name().to_string(), c))
        .collect();

    info!("OpenCrab is ready 🦀");

    while let Some(message) = rx.recv().await {
        let agent = agent.clone();
        let channel_name = message.channel.clone();
        let chat_id = message.chat_id.clone();
        let channels_map = channels_map.clone();

        tokio::spawn(async move {
            info!(channel = %channel_name, sender = %message.sender, "Received message");

            let channel = match channels_map.get(&channel_name) {
                Some(ch) => ch,
                None => {
                    error!(channel = %channel_name, "Channel not found");
                    return;
                }
            };

            if message.metadata.get("command").map(|c| c.as_str()) == Some("clear_history") {
                let memory = agent.memory();
                let chat_id = message.chat_id.clone();
                let ch = channel.clone();
                if let Err(e) = memory.clear_history(&chat_id).await {
                    error!(error = %e, "Failed to clear history");
                } else {
                    let _ = ch.send_message(&chat_id, "History cleared.").await;
                }
                return;
            }

            if channel.supports_streaming() {
                let ch = channel.clone();
                let cid = chat_id.clone();

                let _ = ch.send_stream_start(&cid).await;

                let result = agent
                    .handle_message_streaming(message, |chunk| {
                        let ch = ch.clone();
                        let cid = cid.clone();
                        let chunk = chunk.to_string();
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                let _ = ch.send_stream_chunk(&cid, &chunk).await;
                            });
                        });
                    })
                    .await;

                let _ = ch.send_stream_end(&cid).await;

                if let Err(e) = result {
                    error!(error = %e, "Agent streaming error");
                }
            } else {
                match agent.handle_message(message).await {
                    Ok(response) => {
                        if let Err(e) = channel.send_message(&chat_id, &response).await {
                            error!(error = %e, "Failed to send response");
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Agent error");
                        let _ = channel.send_message(&chat_id, &format!("Sorry, an error occurred: {e}")).await;
                    }
                }
            }
        });
    }

    Ok(())
}
