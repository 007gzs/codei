//! SDK usage example (requires OPENAI_API_KEY).
//!
//! Run with:
//!   cargo run -p codei-sdk --example run_prompt -- "list rust files"

use codei_sdk::CodeiClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "List all Rust source files in the current directory.".into());

    let client = CodeiClient::builder().auto_approve(true).build().await?;

    let result = client
        .run_with_handler(&prompt, |event| match event {
            codei_agent::AgentEvent::AssistantDelta { text } => print!("{text}"),
            codei_agent::AgentEvent::ToolStarted { name, args } => {
                eprintln!("\n[tool:{name}] {args}");
            }
            codei_agent::AgentEvent::ToolFinished { name, result } => {
                eprintln!("[tool:{name}] {}", result.content);
            }
            _ => {}
        })
        .await?;

    println!("\n\nsession={}", result.session_id);
    Ok(())
}
