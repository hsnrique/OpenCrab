use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::{
    ChatMessage, MemoryStore, Message, Provider, StreamChunk, Tool, ToolCall, ToolDef,
    config::Config,
};

pub struct Agent {
    config: Config,
    provider: Arc<dyn Provider>,
    memory: Arc<dyn MemoryStore>,
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl Agent {
    pub fn new(
        config: Config,
        provider: Arc<dyn Provider>,
        memory: Arc<dyn MemoryStore>,
    ) -> Self {
        Self {
            config,
            provider,
            memory,
            tools: HashMap::new(),
        }
    }

    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        info!(tool = %name, "Registered tool");
        self.tools.insert(name, tool);
    }

    pub fn memory(&self) -> &Arc<dyn MemoryStore> {
        &self.memory
    }

    pub async fn handle_message(&self, message: Message) -> Result<String> {
        let chat_id = &message.chat_id;

        let user_msg = ChatMessage::user(&message.content);
        self.memory.save_message(chat_id, &user_msg).await?;

        let history = self
            .memory
            .get_history(chat_id, 50)
            .await
            .unwrap_or_default();

        let facts = self
            .memory
            .get_facts(&message.sender)
            .await
            .unwrap_or_default();

        let system_prompt = self.build_system_prompt(&facts);
        let tool_defs = self.collect_tool_definitions();

        let mut messages = vec![ChatMessage::system(&system_prompt)];
        messages.extend(history);

        let response = self.tool_loop(&mut messages, &tool_defs).await?;

        let assistant_msg = ChatMessage::assistant(&response);
        self.memory.save_message(chat_id, &assistant_msg).await?;

        Ok(response)
    }

    pub async fn handle_message_streaming<F>(
        &self,
        message: Message,
        on_chunk: F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        let chat_id = &message.chat_id;

        let user_msg = ChatMessage::user(&message.content);
        self.memory.save_message(chat_id, &user_msg).await?;

        let history = self
            .memory
            .get_history(chat_id, 50)
            .await
            .unwrap_or_default();

        let facts = self
            .memory
            .get_facts(&message.sender)
            .await
            .unwrap_or_default();

        let system_prompt = self.build_system_prompt(&facts);
        let tool_defs = self.collect_tool_definitions();

        let mut messages = vec![ChatMessage::system(&system_prompt)];
        messages.extend(history);

        let response = self
            .tool_loop_streaming(&mut messages, &tool_defs, &on_chunk)
            .await?;

        let assistant_msg = ChatMessage::assistant(&response);
        self.memory.save_message(chat_id, &assistant_msg).await?;

        Ok(response)
    }

    async fn tool_loop(
        &self,
        messages: &mut Vec<ChatMessage>,
        tool_defs: &[ToolDef],
    ) -> Result<String> {
        let max_iterations = self.config.agent.max_tool_iterations;

        for iteration in 0..max_iterations {
            debug!(iteration, "Agent loop iteration");

            let response = self
                .provider
                .complete(messages, tool_defs)
                .await
                .context("Provider completion failed")?;

            if response.tool_calls.is_empty() {
                return Ok(response.content);
            }

            let assistant_msg = ChatMessage {
                role: crate::Role::Assistant,
                content: response.content.clone(),
                tool_call_id: None,
                tool_calls: response.tool_calls.clone(),
            };
            messages.push(assistant_msg);

            for tool_call in &response.tool_calls {
                let result = self.execute_tool(tool_call).await;
                let tool_msg = ChatMessage::tool_result(&tool_call.id, &result);
                messages.push(tool_msg);
            }
        }

        warn!("Reached max tool iterations ({})", max_iterations);
        Ok("I've reached the maximum number of actions I can take in a single turn. Here's what I've done so far — let me know if you'd like me to continue.".to_string())
    }

    async fn tool_loop_streaming<F>(
        &self,
        messages: &mut Vec<ChatMessage>,
        tool_defs: &[ToolDef],
        on_chunk: &F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        let max_iterations = self.config.agent.max_tool_iterations;

        for iteration in 0..max_iterations {
            debug!(iteration, "Streaming agent loop iteration");

            if iteration == 0 {
                let mut rx = self
                    .provider
                    .stream(messages, tool_defs)
                    .await
                    .context("Provider stream failed")?;

                let mut full_content = String::new();
                let mut pending_tool_calls: HashMap<String, (String, String)> = HashMap::new();
                let mut completed_tool_calls: Vec<ToolCall> = Vec::new();

                while let Some(chunk) = rx.recv().await {
                    match chunk {
                        StreamChunk::Delta(text) => {
                            on_chunk(&text);
                            full_content.push_str(&text);
                        }
                        StreamChunk::ToolCallStart { id, name } => {
                            pending_tool_calls.insert(id, (name, String::new()));
                        }
                        StreamChunk::ToolCallDelta { id, arguments_delta } => {
                            if let Some(entry) = pending_tool_calls.get_mut(&id) {
                                entry.1.push_str(&arguments_delta);
                            }
                        }
                        StreamChunk::ToolCallEnd { id, name, arguments, thought_signature } => {
                            pending_tool_calls.remove(&id);
                            completed_tool_calls.push(ToolCall { id, name, arguments, thought_signature });
                        }
                        StreamChunk::Done => break,
                        StreamChunk::Error(e) => {
                            anyhow::bail!("Stream error: {e}");
                        }
                    }
                }

                for (id, (name, args_str)) in &pending_tool_calls {
                    let arguments = serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                    completed_tool_calls.push(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments,
                        thought_signature: None,
                    });
                }

                if completed_tool_calls.is_empty() {
                    return Ok(full_content);
                }

                let assistant_msg = ChatMessage {
                    role: crate::Role::Assistant,
                    content: full_content,
                    tool_call_id: None,
                    tool_calls: completed_tool_calls.clone(),
                };
                messages.push(assistant_msg);

                for tool_call in &completed_tool_calls {
                    let result = self.execute_tool(tool_call).await;
                    let tool_msg = ChatMessage::tool_result(&tool_call.id, &result);
                    messages.push(tool_msg);
                }
            } else {
                let response = self
                    .provider
                    .complete(messages, tool_defs)
                    .await
                    .context("Provider completion failed after tool execution")?;

                if response.tool_calls.is_empty() {
                    on_chunk(&response.content);
                    return Ok(response.content);
                }

                let assistant_msg = ChatMessage {
                    role: crate::Role::Assistant,
                    content: response.content.clone(),
                    tool_call_id: None,
                    tool_calls: response.tool_calls.clone(),
                };
                messages.push(assistant_msg);

                for tool_call in &response.tool_calls {
                    let result = self.execute_tool(tool_call).await;
                    let tool_msg = ChatMessage::tool_result(&tool_call.id, &result);
                    messages.push(tool_msg);
                }
            }
        }

        warn!("Reached max tool iterations ({})", max_iterations);
        Ok("I've reached the maximum number of actions I can take. Let me know if you'd like me to continue.".to_string())
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> String {
        let tool_name = &tool_call.name;

        let Some(tool) = self.tools.get(tool_name) else {
            warn!(tool = %tool_name, "Unknown tool requested");
            return format!("Error: unknown tool '{tool_name}'");
        };

        info!(tool = %tool_name, "Executing tool");

        match tool.execute(tool_call.arguments.clone()).await {
            Ok(result) => {
                debug!(tool = %tool_name, result_len = result.len(), "Tool executed");
                result
            }
            Err(err) => {
                warn!(tool = %tool_name, error = %err, "Tool execution failed");
                format!("Error executing {tool_name}: {err}")
            }
        }
    }

    fn build_system_prompt(&self, facts: &HashMap<String, String>) -> String {
        let mut prompt = self.config.agent.system_prompt.clone();

        if !facts.is_empty() {
            prompt.push_str("\n\nHere's what you know about this user:\n");
            for (key, value) in facts {
                prompt.push_str(&format!("- {key}: {value}\n"));
            }
        }

        prompt
    }

    fn collect_tool_definitions(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.definition()).collect()
    }
}
