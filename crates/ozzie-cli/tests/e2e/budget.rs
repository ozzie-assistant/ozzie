use std::sync::Arc;
use std::time::Duration;

use ozzie_client::OpenSessionOpts;
use ozzie_tools::ToolRegistry;

use crate::harness::{
    collect_until_assistant_message, extract_assistant_text, TestGateway, TestGatewayConfig,
};

/// Tests that a conversation with all tools registered completes normally.
#[tokio::test]
async fn conversation_with_all_tools_completes() {
    require_provider!(provider);

    let registry = Arc::new(ToolRegistry::new());
    ozzie_tools::native::register_all(&registry, None);
    let tools = registry.all_tools();

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools,
        blob_store: None,
    })
    .await;

    let mut client = gw.connect().await;
    client
        .open_session(OpenSessionOpts {
            session_id: None,
            working_dir: None,
        })
        .await
        .expect("open session");

    client
        .send_message("Say hello briefly.")
        .await
        .expect("send message");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;

    let text = extract_assistant_text(&frames).expect("should get assistant response");
    assert!(!text.is_empty(), "response should not be empty");
}
