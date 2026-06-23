use codei_config::McpServer;
use codei_mcp::McpClient;
use serde_json::json;

#[tokio::test]
async fn lists_and_calls_tools() {
    let server = McpServer {
        name: "test".into(),
        command: env!("CARGO_BIN_EXE_mcp-test-server").into(),
        args: Vec::new(),
        env: Vec::new(),
    };

    let mut client = McpClient::connect(&server).await.expect("connect");
    let tools = client.list_tools().await.expect("list tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");

    let result = client
        .call_tool("echo", json!({ "message": "hello" }))
        .await
        .expect("call");
    assert!(result.text().contains("hello"));
}
