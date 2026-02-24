<div align="center">
  <h1>🦀 OpenCrab</h1>
  <p><strong>A modular AI assistant that runs on your machine.</strong></p>
  <p>Multi-provider • Multi-channel • Streaming • Tool-use • Local-first</p>

  <br/>

  ![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
  ![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)
</div>

---

## What is OpenCrab?

OpenCrab is a personal AI assistant built in Rust that runs entirely on your local machine. It connects to LLM providers (Gemini, OpenAI, Anthropic), receives messages through multiple channels (CLI, Telegram, Discord, WhatsApp), and uses tools to interact with your system.

```
You: list the rust files in my project
🦀 [executes `find . -name "*.rs"` via shell tool]
   Found 14 Rust source files:
   ./src/main.rs
   ./crates/core/src/lib.rs
   ...
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                     OpenCrab                         │
│                                                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │
│  │   CLI    │  │ Telegram │  │ Discord/WhatsApp │   │
│  └────┬─────┘  └────┬─────┘  └────────┬─────────┘   │
│       └──────────────┼────────────────┘              │
│                      ▼                               │
│              ┌──────────────┐                        │
│              │    Agent     │ ◄── System Prompt       │
│              │  (Core Loop) │ ◄── Memory (SQLite)    │
│              └──────┬───────┘                        │
│                     │                                │
│       ┌─────────────┼─────────────┐                  │
│       ▼             ▼             ▼                  │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐           │
│  │ Gemini  │  │  OpenAI  │  │ Anthropic │           │
│  └─────────┘  └──────────┘  └───────────┘           │
│                     │                                │
│       ┌─────────────┼─────────────────┐              │
│       ▼             ▼             ▼   ▼              │
│  ┌────────┐  ┌────────────┐  ┌─────┐ ┌──────┐       │
│  │ Shell  │  │ Filesystem │  │ URL │ │ HTTP │       │
│  └────────┘  └────────────┘  └─────┘ └──────┘       │
└─────────────────────────────────────────────────────┘
```

## Features

| Feature | Status |
|---------|--------|
| **Streaming responses** | ✅ Real-time token output |
| **Multi-provider** | ✅ Gemini, OpenAI, Anthropic |
| **Tool calling** | ✅ Shell, Filesystem, URL, HTTP, Code Runner, System Info |
| **Web Search** | ✅ Native Gemini Google Search grounding |
| **CLI channel** | ✅ Interactive terminal |
| **Telegram bot** | ✅ Full integration |
| **Discord bot** | ✅ Serenity gateway |
| **WhatsApp** | ✅ Webhook + Cloud API |
| **Memory** | ✅ SQLite conversation persistence |
| **Browser automation** | ✅ Headless Chrome via chromiumoxide |
| **WASM plugins** | ✅ Wasmtime runtime, auto-load from plugins/ |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- A Gemini, OpenAI, or Anthropic API key

### Setup

```bash
git clone https://github.com/hsnrique/OpenCrab.git
cd OpenCrab

# Copy and edit configuration
cp config.toml.example config.toml  # or create your own
# Edit config.toml with your API key

# Run
cargo run
```

### Configuration

Create a `config.toml` at the project root:

```toml
[agent]
name = "OpenCrab"
default_provider = "gemini"    # gemini | openai | anthropic
max_tool_iterations = 10
system_prompt = """
You are OpenCrab, a personal AI assistant.
"""

[providers.gemini]
api_key = "YOUR_GEMINI_API_KEY"
model = "gemini-3-flash-preview"

# [providers.openai]
# api_key = "YOUR_OPENAI_KEY"
# model = "gpt-4o"

# [providers.anthropic]
# api_key = "YOUR_ANTHROPIC_KEY"
# model = "claude-sonnet-4-20250514"

[channels.cli]
enabled = true

# [channels.telegram]
# enabled = true
# bot_token = "YOUR_BOT_TOKEN"

[memory]
database_path = "./data/opencrab.db"

[tools]
shell_enabled = true
filesystem_enabled = true
web_search_enabled = true   # Uses Gemini's native Google Search grounding
url_reader_enabled = true
http_enabled = true
web_search_enabled = true
# google_search_api_key = "YOUR_KEY"
# google_search_cx = "YOUR_CX"
```

## Tools

| Tool | Description |
|------|-------------|
| `shell` | Execute shell commands on the local system |
| `filesystem` | Read, write, and list files and directories |
| `url_reader` | Fetch and extract text content from web pages |
| `http_request` | Make HTTP requests (GET, POST, PUT, DELETE) to APIs |
| `web_search` | Native Google Search grounding via Gemini (no extra API key) |
| `code_runner` | Execute Python, Node.js, or Bash code snippets |
| `system_info` | Get OS details, processes, disk, network, and env info |
| `browser` | Headless Chrome: navigate, screenshot, extract text, click, type, eval JS |

## Project Structure

```
OpenCrab/
├── src/main.rs                    # Entry point, wiring
├── crates/
│   ├── core/                      # Agent, traits, config
│   ├── providers/
│   │   ├── gemini/                # Google Gemini
│   │   ├── openai/                # OpenAI / GPT
│   │   └── anthropic/             # Anthropic / Claude
│   ├── channels/
│   │   ├── cli/                   # Terminal interface
│   │   ├── telegram/              # Telegram bot
│   │   ├── discord/               # Discord bot
│   │   └── whatsapp/              # WhatsApp webhook
│   ├── tools/                     # Shell, FS, URL, HTTP, Code Runner, System Info
│   ├── tools-browser/             # Headless Chrome automation
│   ├── plugin-wasm/               # WASM plugin runtime (wasmtime)
│   └── memory/                    # SQLite storage
├── plugins/                       # WASM plugins auto-loaded here
└── config.toml                    # Runtime configuration
```

## Commands

When using the CLI channel:

| Command | Action |
|---------|--------|
| `/quit` or `/exit` | Exit OpenCrab |
| `/clear` | Clear conversation history |

## License

MIT

---

<div align="center">
  <sub>Built with 🦀 and ❤️</sub>
</div>
