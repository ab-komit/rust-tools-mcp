use libloading::Library;
use mcp_plugin_types::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── MCP protocol types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Serialize)]
struct InitializeResult {
    protocol_version: String,
    capabilities: serde_json::Value,
    server_info: ServerInfo,
}

// ── Plugin manager ──────────────────────────────────────────────────────

struct LoadedPlugin {
    _name: String,
    _lib: Library,
    tools: Vec<ToolDescriptor>,
    call_tool:
        unsafe extern "C" fn(*const std::os::raw::c_char, *const std::os::raw::c_char) -> *mut std::os::raw::c_char,
    free_string: unsafe extern "C" fn(*mut std::os::raw::c_char),
}

unsafe impl Send for LoadedPlugin {}
unsafe impl Sync for LoadedPlugin {}

struct PluginManager {
    tool_to_plugin: HashMap<String, Arc<LoadedPlugin>>,
    all_tools: Vec<ToolDescriptor>,
}

impl PluginManager {
    fn load(dir: &Path) -> Self {
        let mut tool_to_plugin: HashMap<String, Arc<LoadedPlugin>> = HashMap::new();

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => {
                return Self {
                    tool_to_plugin,
                    all_tools: Vec::new(),
                };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("so" | "dylib" | "dll")) {
                continue;
            }

            match Self::load_plugin(&path) {
                Ok(plugin) => {
                    let plugin = Arc::new(plugin);
                    for tool in &plugin.tools {
                        tool_to_plugin.insert(tool.name.clone(), Arc::clone(&plugin));
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin {:?}: {}", path, e);
                }
            }
        }

        let unique_plugins: HashSet<_> = tool_to_plugin
            .values()
            .map(|p| Arc::as_ptr(p) as usize)
            .collect();

        let all_tools: Vec<ToolDescriptor> = tool_to_plugin
            .values()
            .flat_map(|p| p.tools.clone())
            .collect();

        tracing::info!(
            "Loaded {} plugins with {} tools from {:?}",
            unique_plugins.len(),
            all_tools.len(),
            dir
        );

        Self {
            tool_to_plugin,
            all_tools,
        }
    }

    fn load_plugin(path: &Path) -> Result<LoadedPlugin, Box<dyn std::error::Error>> {
        unsafe {
            let lib = Library::new(path)?;

            let abi_version: libloading::Symbol<unsafe extern "C" fn() -> u32> =
                lib.get(b"plugin_abi_version")?;
            if abi_version() != ABI_VERSION {
                return Err(format!("Unsupported ABI version: expected {ABI_VERSION}").into());
            }
            drop(abi_version);

            let list_tools_fn: libloading::Symbol<unsafe extern "C" fn() -> *mut std::os::raw::c_char> =
                lib.get(b"plugin_list_tools")?;
            let tools_json = c_str_to_string(list_tools_fn());
            let tools: Vec<ToolDescriptor> = serde_json::from_str(&tools_json)?;
            drop(list_tools_fn);

            let plugin_name_fn: libloading::Symbol<unsafe extern "C" fn() -> *mut std::os::raw::c_char> =
                lib.get(b"plugin_name")?;
            let name_ptr = plugin_name_fn();
            let name = c_str_to_string(name_ptr);

            let call_tool: libloading::Symbol<
                unsafe extern "C" fn(
                    *const std::os::raw::c_char,
                    *const std::os::raw::c_char,
                ) -> *mut std::os::raw::c_char,
            > = lib.get(b"plugin_call_tool")?;
            let call_tool = *call_tool;

            let free_string: libloading::Symbol<
                unsafe extern "C" fn(*mut std::os::raw::c_char),
            > = lib.get(b"plugin_free_string")?;
            let free_string = *free_string;

            (free_string)(name_ptr);
            drop(plugin_name_fn);

            Ok(LoadedPlugin {
                _name: name,
                _lib: lib,
                tools,
                call_tool,
                free_string,
            })
        }
    }

    fn list_tools(&self) -> Vec<serde_json::Value> {
        self.all_tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect()
    }

    fn call_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolResult, String> {
        let plugin = self
            .tool_to_plugin
            .get(name)
            .ok_or_else(|| format!("Unknown tool: {name}"))?;

        let args_json = serde_json::to_string(args).unwrap_or_default();
        let c_name = std::ffi::CString::new(name).unwrap();
        let c_args = std::ffi::CString::new(args_json).unwrap();

        unsafe {
            let result_ptr = (plugin.call_tool)(c_name.as_ptr(), c_args.as_ptr());
            let result_str = c_str_to_string(result_ptr);
            (plugin.free_string)(result_ptr);
            serde_json::from_str(&result_str)
                .map_err(|e| format!("Failed to parse tool result: {e}"))
        }
    }
}

// ── Directory paths ─────────────────────────────────────────────────────

fn default_global_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("rust-tools")
}

fn default_project_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(".rust-tools")
}

// ── MCP server ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let global_dir = default_global_dir();
    let project_dir = default_project_dir();

    let mut mgr = PluginManager::load(&global_dir);

    if project_dir != global_dir {
        let project_mgr = PluginManager::load(&project_dir);
        for plugin in project_mgr.tool_to_plugin.into_values() {
            for tool in &plugin.tools {
                mgr.tool_to_plugin.insert(tool.name.clone(), Arc::clone(&plugin));
            }
        }
        mgr.all_tools = mgr
            .tool_to_plugin
            .values()
            .flat_map(|p| p.tools.clone())
            .collect::<Vec<_>>();
        mgr.all_tools.sort_by(|a, b| a.name.cmp(&b.name));
        mgr.all_tools.dedup_by_key(|t| t.name.clone());
    }

    tracing::info!("mcp-host ready with {} tools", mgr.list_tools().len());

    run_mcp_server(&mgr).await;
}

async fn run_mcp_server(mgr: &PluginManager) {
    let stdin = io::stdin();
    let reader = stdin.lock();
    let mut lines = reader.lines();

    while let Some(Ok(line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Invalid JSON-RPC: {e}");
                continue;
            }
        };

        let is_notification = request.id.is_none();
        let id = request.id;

        let response = handle_request(mgr, &request.method, request.params.as_ref(), id).await;

        // Notifications get no response
        if is_notification {
            continue;
        }

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        let json = serde_json::to_string(&response).unwrap();
        writeln!(handle, "{json}").ok();
        handle.flush().ok();
    }
}

async fn handle_request(
    mgr: &PluginManager,
    method: &str,
    params: Option<&serde_json::Value>,
    id: Option<serde_json::Value>,
) -> serde_json::Value {
    match method {
        "initialize" => {
            let result = InitializeResult {
                protocol_version: "2024-11-05".into(),
                capabilities: serde_json::json!({
                    "tools": {}
                }),
                server_info: ServerInfo {
                    name: "mcp-host".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            };
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })
        }

        "tools/list" => {
            let tools = mgr.list_tools();
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tools },
            })
        }

        "tools/call" => {
            let params = params
                .and_then(|p| p.as_object())
                .cloned()
                .unwrap_or_default();
            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            match mgr.call_tool(&tool_name, &arguments) {
                Ok(result) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": result.content,
                            "isError": result.is_error,
                        },
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": e}],
                            "isError": true,
                        },
                    })
                }
            }
        }

        _ => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}"),
                },
            })
        }
    }
}
