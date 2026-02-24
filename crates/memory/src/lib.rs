use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use tracing::info;

use opencrab_core::{ChatMessage, MemoryStore, Role};

pub struct SqliteMemory {
    conn: Mutex<Connection>,
}

impl SqliteMemory {
    pub fn new(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.initialize_tables()?;

        info!(path = %db_path.display(), "Memory store initialized");
        Ok(store)
    }

    fn initialize_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_call_id TEXT,
                tool_calls TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_messages_chat_id
                ON messages(chat_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS facts (
                user_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (user_id, key)
            );",
        )?;

        Ok(())
    }
}

#[async_trait]
impl MemoryStore for SqliteMemory {
    async fn save_message(&self, chat_id: &str, msg: &ChatMessage) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        let role_str = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        let tool_calls_json = if msg.tool_calls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&msg.tool_calls)?)
        };

        conn.execute(
            "INSERT INTO messages (chat_id, role, content, tool_call_id, tool_calls)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                chat_id,
                role_str,
                msg.content,
                msg.tool_call_id,
                tool_calls_json,
            ],
        )?;

        Ok(())
    }

    async fn get_history(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls
             FROM messages
             WHERE chat_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![chat_id, limit], |row| {
            let role_str: String = row.get(0)?;
            let content: String = row.get(1)?;
            let tool_call_id: Option<String> = row.get(2)?;
            let tool_calls_json: Option<String> = row.get(3)?;

            Ok((role_str, content, tool_call_id, tool_calls_json))
        })?;

        let mut messages: Vec<ChatMessage> = Vec::new();

        for row in rows {
            let (role_str, content, tool_call_id, tool_calls_json) = row?;

            let role = match role_str.as_str() {
                "system" => Role::System,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => Role::User,
            };

            let tool_calls = match tool_calls_json {
                Some(json) => serde_json::from_str(&json).unwrap_or_default(),
                None => vec![],
            };

            messages.push(ChatMessage {
                role,
                content,
                tool_call_id,
                tool_calls,
            });
        }

        messages.reverse();
        Ok(messages)
    }

    async fn save_fact(&self, user_id: &str, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO facts (user_id, key, value, updated_at)
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(user_id, key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP",
            rusqlite::params![user_id, key, value],
        )?;

        Ok(())
    }

    async fn get_facts(&self, user_id: &str) -> Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT key, value FROM facts WHERE user_id = ?1",
        )?;

        let rows = stmt.query_map(rusqlite::params![user_id], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;

        let mut facts = HashMap::new();
        for row in rows {
            let (key, value) = row?;
            facts.insert(key, value);
        }

        Ok(facts)
    }

    async fn clear_history(&self, chat_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE chat_id = ?1", rusqlite::params![chat_id])?;
        Ok(())
    }
}
