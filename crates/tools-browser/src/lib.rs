use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, info};

use opencrab_core::{Tool, ToolDef};

pub struct BrowserTool {
    browser: Arc<Mutex<Option<Arc<Browser>>>>,
}

impl Default for BrowserTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserTool {
    pub fn new() -> Self {
        Self {
            browser: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_browser(&self) -> Result<Arc<Browser>> {
        let mut guard = self.browser.lock().await;
        if let Some(browser) = guard.as_ref() {
            return Ok(browser.clone());
        }

        info!("Launching headless browser...");
        let (browser, mut handler) = Browser::launch(
            BrowserConfig::builder()
                .no_sandbox()
                .build()
                .map_err(|e| anyhow::anyhow!("Browser config error: {e}"))?,
        )
        .await
        .context("Failed to launch browser")?;

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                debug!(?event, "Browser event");
            }
        });

        let browser = Arc::new(browser);
        *guard = Some(browser.clone());
        Ok(browser)
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "browser".to_string(),
            description: "Control a headless browser to navigate web pages, take screenshots, extract text content, or interact with elements. Useful for reading dynamic/JavaScript-heavy pages.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "The browser action to perform",
                        "enum": ["navigate", "screenshot", "get_text", "click", "type_text", "evaluate"]
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to (for 'navigate' action)"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for element interaction"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type (for 'type_text' action)"
                    },
                    "script": {
                        "type": "string",
                        "description": "JavaScript to evaluate (for 'evaluate' action)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let action = params["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let browser = match self.get_browser().await {
            Ok(b) => b,
            Err(e) => return Ok(format!("Failed to launch browser: {e}. Make sure Chrome/Chromium is installed.")),
        };

        match action {
            "navigate" => {
                let url = params["url"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'url' for navigate"))?;

                info!(url, "Navigating browser");
                let page = browser.new_page(url).await
                    .context("Failed to navigate")?;

                page.wait_for_navigation().await.ok();
                let title = page.get_title().await?.unwrap_or_default();

                Ok(format!("Navigated to: {url}\nTitle: {title}"))
            }

            "screenshot" => {
                let pages = browser.pages().await?;
                let page = pages.last()
                    .ok_or_else(|| anyhow::anyhow!("No page open. Use 'navigate' first."))?;

                let screenshot = page.screenshot(
                    ScreenshotParams::builder()
                        .full_page(true)
                        .build(),
                ).await?;

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let path = format!("/tmp/opencrab_screenshot_{timestamp}.png");
                std::fs::write(&path, &screenshot)?;

                Ok(format!("Screenshot saved to: {path} ({} bytes)", screenshot.len()))
            }

            "get_text" => {
                let pages = browser.pages().await?;
                let page = pages.last()
                    .ok_or_else(|| anyhow::anyhow!("No page open. Use 'navigate' first."))?;

                let selector = params["selector"].as_str();

                let text = if let Some(sel) = selector {
                    let element = page.find_element(sel).await
                        .context("Element not found")?;
                    element.inner_text().await?.unwrap_or_default()
                } else {
                    let body = page.find_element("body").await?;
                    let full = body.inner_text().await?.unwrap_or_default();
                    if full.len() > 10000 {
                        format!("{}...\n(truncated, {} total chars)", &full[..10000], full.len())
                    } else {
                        full
                    }
                };

                Ok(text)
            }

            "click" => {
                let selector = params["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'selector' for click"))?;

                let pages = browser.pages().await?;
                let page = pages.last()
                    .ok_or_else(|| anyhow::anyhow!("No page open."))?;

                page.find_element(selector).await
                    .context("Element not found")?
                    .click().await?;

                Ok(format!("Clicked element: {selector}"))
            }

            "type_text" => {
                let selector = params["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'selector'"))?;
                let text = params["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'text'"))?;

                let pages = browser.pages().await?;
                let page = pages.last()
                    .ok_or_else(|| anyhow::anyhow!("No page open."))?;

                page.find_element(selector).await
                    .context("Element not found")?
                    .click().await?
                    .type_str(text).await?;

                Ok(format!("Typed into {selector}"))
            }

            "evaluate" => {
                let script = params["script"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'script' for evaluate"))?;

                let pages = browser.pages().await?;
                let page = pages.last()
                    .ok_or_else(|| anyhow::anyhow!("No page open."))?;

                let result: serde_json::Value = page.evaluate(script).await
                    .context("JavaScript execution failed")?
                    .into_value()?;

                Ok(serde_json::to_string_pretty(&result)?)
            }

            _ => Ok(format!("Unknown action: {action}")),
        }
    }
}
