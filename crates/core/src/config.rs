use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub providers: ProvidersConfig,
    pub channels: ChannelsConfig,
    pub memory: MemoryConfig,
    pub tools: ToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub default_provider: String,
    pub system_prompt: String,
    pub max_tool_iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub gemini: Option<ProviderEntry>,
    #[serde(default)]
    pub openai: Option<ProviderEntry>,
    #[serde(default)]
    pub anthropic: Option<ProviderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub cli: Option<CliChannelConfig>,
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
    #[serde(default)]
    pub whatsapp: Option<WhatsAppConfig>,
    #[serde(default)]
    pub discord: Option<DiscordConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliChannelConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    pub enabled: bool,
    pub phone_number_id: String,
    pub access_token: String,
    pub verify_token: String,
    #[serde(default = "default_webhook_port")]
    pub webhook_port: u16,
}

fn default_webhook_port() -> u16 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub bot_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub database_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub shell_enabled: bool,
    #[serde(default = "default_true")]
    pub filesystem_enabled: bool,
    #[serde(default)]
    pub browser_enabled: bool,
    #[serde(default)]
    pub filesystem_root: Option<String>,
    #[serde(default)]
    pub shell_allowed_commands: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn default_config() -> Self {
        Self {
            agent: AgentConfig {
                name: "OpenCrab".to_string(),
                default_provider: "gemini".to_string(),
                system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
                max_tool_iterations: 10,
            },
            providers: ProvidersConfig {
                gemini: None,
                openai: None,
                anthropic: None,
            },
            channels: ChannelsConfig {
                cli: Some(CliChannelConfig { enabled: true }),
                telegram: None,
                whatsapp: None,
                discord: None,
            },
            memory: MemoryConfig {
                database_path: PathBuf::from("./data/opencrab.db"),
            },
            tools: ToolsConfig {
                shell_enabled: true,
                filesystem_enabled: true,
                browser_enabled: false,
                filesystem_root: None,
                shell_allowed_commands: vec![],
            },
        }
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are OpenCrab, a personal AI assistant that runs locally on the user's machine.

You have access to tools that let you interact with the user's system:
- Execute shell commands
- Read and write files
- Browse the web

When the user asks you to do something, use the appropriate tools to actually perform the action.
Be concise, helpful, and proactive. If a task requires multiple steps, execute them in sequence.
Always confirm what you did after completing an action.

Remember context from previous conversations to provide a personalized experience."#;
