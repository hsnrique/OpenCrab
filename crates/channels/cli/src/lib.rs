use std::io::{self, BufRead, Write};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use opencrab_core::{Channel, Message};

pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

fn print_prompt() {
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "> ");
    let _ = stdout.flush();
}

#[async_trait]
impl Channel for CliChannel {
    async fn start(&self, tx: mpsc::UnboundedSender<Message>) -> Result<()> {
        eprintln!();
        eprintln!("🦀 OpenCrab — Personal AI Assistant");
        eprintln!("   Type your message and press Enter.");
        eprintln!("   Type /quit to exit, /clear to reset history.");
        eprintln!();
        print_prompt();

        let tx_clone = tx.clone();

        tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let reader = stdin.lock();

            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };

                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    print_prompt();
                    continue;
                }

                if trimmed == "/quit" || trimmed == "/exit" {
                    eprintln!("🦀 Bye!");
                    std::process::exit(0);
                }

                if trimmed == "/clear" {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("command".to_string(), "clear_history".to_string());
                    let mut msg = Message::new("cli", "cli-session", "user", "/clear");
                    msg.metadata = meta;
                    if tx_clone.send(msg).is_err() { break; }
                    continue;
                }

                let msg = Message::new("cli", "cli-session", "user", &trimmed);
                if tx_clone.send(msg).is_err() { break; }
            }
        });

        Ok(())
    }

    async fn send_message(&self, _chat_id: &str, content: &str) -> Result<()> {
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
        let _ = writeln!(stderr, "🦀 {content}");
        let _ = writeln!(stderr);
        print_prompt();
        Ok(())
    }

    async fn send_stream_start(&self, _chat_id: &str) -> Result<()> {
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
        let _ = write!(stderr, "🦀 ");
        let _ = stderr.flush();
        Ok(())
    }

    async fn send_stream_chunk(&self, _chat_id: &str, chunk: &str) -> Result<()> {
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "{chunk}");
        let _ = stderr.flush();
        Ok(())
    }

    async fn send_stream_end(&self, _chat_id: &str) -> Result<()> {
        let mut stderr = io::stderr().lock();
        let _ = writeln!(stderr);
        let _ = writeln!(stderr);
        print_prompt();
        Ok(())
    }

    fn name(&self) -> &str {
        "cli"
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}
