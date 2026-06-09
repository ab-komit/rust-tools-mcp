# rust-tools-mcp

An **MCP server** that loads Rust-based tool plugins as `cdylib` shared libraries. Drop `*.so`/`*.dylib`/`*.dll` files into `~/.config/rust-tools/` and they're automatically registered as MCP tools with typed JSON Schemas.

Works with any MCP client: opencode, Claude Desktop, VS Code Copilot, `mcp-cli`, etc.

## Quick start

```bash
# Build the MCP server
cargo build --release -p mcp-host

# Build the example plugin
cargo build --release -p hello-plugin

# Install the plugin
mkdir -p ~/.config/rust-tools
cp target/release/libhello_plugin.so ~/.config/rust-tools/
```

Add to `opencode.json`:

```jsonc
{
  "mcp": {
    "servers": {
      "rust-tools": {
        "command": "/path/to/rust-tools-mcp/target/release/mcp-host"
      }
    }
  }
}
```

## Writing a tool plugin

```rust
use mcp_plugin_sdk::serde::Deserialize;
use mcp_plugin_sdk::schemars::JsonSchema;
use mcp_plugin_sdk_macros::{tool, tool_plugin};

#[derive(Deserialize, JsonSchema)]
struct ReviewArgs {
    /// Path to the file to review
    file_path: String,
    /// Whether to run security checks
    #[schemars(default)]
    check_security: bool,
}

#[tool_plugin]
mod tools {
    use super::*;

    /// Reviews code for security issues
    #[tool]
    fn code_review(args: ReviewArgs) -> Result<String, String> {
        // Tool logic here
        Ok(format!("Reviewed {} (security={})", args.file_path, args.check_security))
    }
}
```

**`Cargo.toml`:**

```toml
[package]
name = "my-tool"

[lib]
crate-type = ["cdylib"]

[dependencies]
mcp-plugin-sdk = { git = "https://github.com/you/rust-tools-mcp" }
mcp-plugin-sdk-macros = { git = "https://github.com/you/rust-tools-mcp" }
serde = { version = "1", features = ["derive"] }
schemars = "0.8"
```

Then build and install:

```bash
cargo build --release
cp target/release/libmy_tool.so ~/.config/rust-tools/
```

## How it works

| Layer | Technology |
|---|---|
| Transport | stdio MCP (JSON-RPC over stdin/stdout) |
| Plugin loading | `libloading` — loads `.so`/`.dylib`/`.dll` |
| Tool schemas | `schemars` — generates JSON Schema from Rust types |
| Plugin ABI | 5 C symbols exported via `#[tool_plugin]` proc macro |
| Runtime | `tokio` (host only) — plugins are sync |

### ABI symbols

Each plugin `.so` exports:

| Symbol | Purpose |
|---|---|
| `plugin_abi_version` | Version check |
| `plugin_name` | Human-readable name |
| `plugin_list_tools` | JSON array of tool descriptors |
| `plugin_call_tool` | Execute a tool by name |
| `plugin_free_string` | Free strings allocated by the plugin |

All generated automatically by the `#[tool_plugin]` proc macro.

### Directory layout

| Scope | Path |
|---|---|
| Global | `~/.config/rust-tools/` |
| Project | `./.rust-tools/` |

Project tools override global tools with the same name.

## Project structure

```
rust-tools-mcp/
├── crates/
│   ├── mcp-host/           # MCP server binary (standalone)
│   ├── mcp-plugin-sdk/     # Re-exports for plugin authors
│   ├── mcp-plugin-sdk-macros/  # #[tool] + #[tool_plugin] proc macros
│   └── mcp-plugin-types/   # Shared ABI types
└── examples/
    └── hello-plugin/       # Example plugin
```

## License

MIT
