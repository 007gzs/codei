//! Minimal MCP stdio server for integration tests.
use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();

        let response = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mcp-test-server", "version": "0.0.1" }
            }),
            "tools/list" => json!({
                "tools": [{
                    "name": "echo",
                    "description": "Echo a message",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" }
                        },
                        "required": ["message"]
                    }
                }]
            }),
            "tools/call" => {
                let name = msg
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = msg
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                if name == "echo" {
                    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": false
                    })
                } else {
                    json!({
                        "content": [{ "type": "text", "text": "unknown tool" }],
                        "isError": true
                    })
                }
            }
            _ => json!({}),
        };

        if let Some(id) = id {
            let out = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": response,
            });
            let mut stdout = io::stdout();
            let _ = writeln!(stdout, "{}", out);
            let _ = stdout.flush();
        }
    }
}
