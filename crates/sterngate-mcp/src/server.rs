use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::debug;

use crate::prompts::{get_prompt_messages, get_prompts_list};
use crate::resources::{get_resources_list, read_resource};
use crate::tools::{get_tools_list, handle_tool_call};

pub struct McpServer;

impl McpServer {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_stdio(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let req: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    let err_resp = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                    });
                    let out = serde_json::to_string(&err_resp)? + "\n";
                    stdout.write_all(out.as_bytes()).await?;
                    stdout.flush().await?;
                    continue;
                }
            };

            let id = req.get("id").cloned();
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let params = req.get("params").cloned().unwrap_or(json!({}));

            // Handle notifications (no id)
            if id.is_none() {
                if method == "notifications/initialized" {
                    debug!("MCP client initialized notification received");
                }
                continue;
            }

            let resp = self.dispatch_method(method, &params).await;
            let final_resp = match resp {
                Ok(result) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                }),
                Err((code, msg)) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": code, "message": msg }
                }),
            };

            let out = serde_json::to_string(&final_resp)? + "\n";
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }

        Ok(())
    }

    async fn dispatch_method(&self, method: &str, params: &Value) -> Result<Value, (i32, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false },
                    "prompts": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "sterngate-mcp",
                    "version": "0.1.0"
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": get_tools_list()
            })),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match handle_tool_call(name, &arguments).await {
                    Ok(val) => Ok(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": serde_json::to_string_pretty(&val).unwrap_or_default()
                            }
                        ]
                    })),
                    Err(e) => Err((-32603, e)),
                }
            }
            "resources/list" => Ok(json!({
                "resources": get_resources_list()
            })),
            "resources/read" => {
                let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                match read_resource(uri) {
                    Ok(content) => Ok(json!({
                        "contents": [
                            {
                                "uri": uri,
                                "mimeType": "application/json",
                                "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                            }
                        ]
                    })),
                    Err(e) => Err((-32002, e)),
                }
            }
            "prompts/list" => Ok(json!({
                "prompts": get_prompts_list()
            })),
            "prompts/get" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match get_prompt_messages(name, &arguments) {
                    Ok(p) => Ok(p),
                    Err(e) => Err((-32602, e)),
                }
            }
            _ => Err((-32601, format!("Method not found: {}", method))),
        }
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}
