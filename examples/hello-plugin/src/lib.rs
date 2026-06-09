use mcp_plugin_sdk::serde::Deserialize;
use mcp_plugin_sdk::schemars::JsonSchema;
use mcp_plugin_sdk::tool_plugin;

#[derive(Deserialize, JsonSchema)]
struct GreetArgs {
    /// The name to greet
    name: String,
    /// Number of times to repeat
    #[schemars(default)]
    repeat: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    /// Text to echo back
    text: String,
    /// Whether to uppercase
    #[schemars(default)]
    uppercase: Option<bool>,
}

#[tool_plugin]
mod tools {
    use super::{GreetArgs, EchoArgs};

    /// Greets a person with style
    #[tool]
    fn greet(args: GreetArgs) -> Result<String, String> {
        let count = args.repeat.unwrap_or(1);
        let mut out = String::new();
        for _ in 0..count {
            out.push_str(&format!("Hello, {}!\n", args.name));
        }
        Ok(out.trim().to_string())
    }

    /// Echoes text back, optionally uppercased
    #[tool]
    fn echo(args: EchoArgs) -> Result<String, String> {
        let result = if args.uppercase.unwrap_or(false) {
            args.text.to_uppercase()
        } else {
            args.text
        };
        Ok(result)
    }
}
