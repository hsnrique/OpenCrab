use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use wasmtime::{Engine, Linker, Module, Store};

use opencrab_core::{Tool, ToolDef};

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub struct WasmPlugin {
    manifest: PluginManifest,
    engine: Engine,
    module: Module,
}

impl WasmPlugin {
    pub fn load(wasm_path: &Path) -> Result<Self> {
        let engine = Engine::default();

        let manifest_path = wasm_path.with_extension("json");
        let manifest: PluginManifest = if manifest_path.exists() {
            let data = std::fs::read_to_string(&manifest_path)
                .context("Failed to read plugin manifest")?;
            serde_json::from_str(&data)
                .context("Failed to parse plugin manifest")?
        } else {
            let name = wasm_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            PluginManifest {
                name: name.clone(),
                description: format!("WASM plugin: {name}"),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string", "description": "Input for the plugin" }
                    },
                    "required": ["input"]
                }),
            }
        };

        let wasm_bytes = std::fs::read(wasm_path)
            .context("Failed to read WASM file")?;
        let module = Module::new(&engine, &wasm_bytes)
            .context("Failed to compile WASM module")?;

        info!(name = %manifest.name, path = %wasm_path.display(), "Loaded WASM plugin");

        Ok(Self {
            manifest,
            engine,
            module,
        })
    }
}

#[async_trait]
impl Tool for WasmPlugin {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: self.manifest.name.clone(),
            description: self.manifest.description.clone(),
            parameters: self.manifest.parameters.clone(),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<String> {
        let input = serde_json::to_string(&params)?;

        let result = tokio::task::spawn_blocking({
            let engine = self.engine.clone();
            let module = self.module.clone();
            let name = self.manifest.name.clone();
            move || -> Result<String> {
                let mut store = Store::new(&engine, ());
                let linker = Linker::new(&engine);

                let instance = linker.instantiate(&mut store, &module)
                    .context("Failed to instantiate WASM module")?;

                let memory = instance.get_memory(&mut store, "memory")
                    .context("WASM module must export 'memory'")?;

                let alloc = instance.get_typed_func::<i32, i32>(&mut store, "alloc")
                    .context("WASM module must export 'alloc(size: i32) -> i32'")?;

                let execute = instance.get_typed_func::<(i32, i32), i32>(&mut store, "execute")
                    .context("WASM module must export 'execute(ptr: i32, len: i32) -> i32'")?;

                let result_len = instance.get_typed_func::<(), i32>(&mut store, "result_len")
                    .context("WASM module must export 'result_len() -> i32'")?;

                let result_ptr = instance.get_typed_func::<(), i32>(&mut store, "result_ptr")
                    .context("WASM module must export 'result_ptr() -> i32'")?;

                let input_bytes = input.as_bytes();
                let ptr = alloc.call(&mut store, input_bytes.len() as i32)?;

                memory.data_mut(&mut store)[ptr as usize..ptr as usize + input_bytes.len()]
                    .copy_from_slice(input_bytes);

                let status = execute.call(&mut store, (ptr, input_bytes.len() as i32))?;

                if status != 0 {
                    return Ok(format!("Plugin '{}' returned error code: {status}", name));
                }

                let rptr = result_ptr.call(&mut store, ())? as usize;
                let rlen = result_len.call(&mut store, ())? as usize;

                let output = &memory.data(&store)[rptr..rptr + rlen];
                let result = String::from_utf8_lossy(output).to_string();

                Ok(result)
            }
        }).await??;

        Ok(result)
    }
}

pub fn load_plugins_from_dir(dir: &Path) -> Vec<WasmPlugin> {
    let mut plugins = Vec::new();

    if !dir.exists() {
        return plugins;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            error!(path = %dir.display(), error = %e, "Failed to read plugins directory");
            return plugins;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            match WasmPlugin::load(&path) {
                Ok(plugin) => {
                    info!(name = %plugin.manifest.name, "Registered WASM plugin");
                    plugins.push(plugin);
                }
                Err(e) => {
                    error!(path = %path.display(), error = %e, "Failed to load WASM plugin");
                }
            }
        }
    }

    plugins
}
