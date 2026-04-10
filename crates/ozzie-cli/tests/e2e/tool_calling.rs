use std::sync::Arc;
use std::time::Duration;

use ozzie_client::OpenSessionOpts;
use ozzie_protocol::EventKind;
use ozzie_tools::ToolRegistry;

use crate::harness::{
    collect_until_assistant_message, count_events, extract_assistant_text, TestGateway,
    TestGatewayConfig,
};

fn file_tools() -> Vec<Arc<dyn ozzie_core::domain::Tool>> {
    let registry = Arc::new(ToolRegistry::new());
    ozzie_tools::native::register_all(&registry, None);
    registry.all_tools()
}

#[tokio::test]
async fn single_tool_file_read() {
    require_provider!(provider);

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools: file_tools(),
        blob_store: None,
    })
    .await;

    // Create a test file
    let test_file = gw.work_dir().join("secret.txt");
    std::fs::write(&test_file, "The answer is forty-two.").expect("write test file");

    let mut client = gw.connect().await;
    client
        .open_session(OpenSessionOpts {
            session_id: None,
            working_dir: Some(gw.work_dir().to_str().unwrap()),
        })
        .await
        .expect("open session");

    let prompt = format!(
        "Read the file at {} and tell me what it says. Be brief.",
        test_file.display()
    );
    client.send_message(&prompt).await.expect("send message");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(180)).await;

    assert!(
        count_events(&frames, EventKind::ToolCall) > 0,
        "should have at least one tool call"
    );

    let text = extract_assistant_text(&frames).unwrap_or_else(|| panic!(
            "no assistant.message frame received ({} frames collected, likely timeout — is Ollama overloaded?)",
            frames.len()
        ));
    let lower = text.to_lowercase();
    assert!(
        lower.contains("forty-two") || lower.contains("42"),
        "response should mention the file content, got: {text}"
    );
}

#[tokio::test]
async fn multi_step_write_then_read() {
    require_provider!(provider);

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools: file_tools(),
        blob_store: None,
    })
    .await;

    let mut client = gw.connect().await;
    client
        .open_session(OpenSessionOpts {
            session_id: None,
            working_dir: Some(gw.work_dir().to_str().unwrap()),
        })
        .await
        .expect("open session");

    let target = gw.work_dir().join("hello.txt");
    let prompt = format!(
        "Create a file at {} with the text 'Hello World', then read it back and confirm the contents.",
        target.display()
    );
    client.send_message(&prompt).await.expect("send message");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(180)).await;

    assert!(
        count_events(&frames, EventKind::ToolCall) >= 2,
        "should have at least 2 tool calls (write + read)"
    );

    let text = extract_assistant_text(&frames).unwrap_or_else(|| panic!(
            "no assistant.message frame received ({} frames collected, likely timeout — is Ollama overloaded?)",
            frames.len()
        ));
    let lower = text.to_lowercase();
    assert!(
        lower.contains("hello world") || lower.contains("hello"),
        "response should confirm file content, got: {text}"
    );
}
