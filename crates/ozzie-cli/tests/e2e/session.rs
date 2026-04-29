use std::time::Duration;

use ozzie_client::OpenConversationOpts;

use crate::harness::{
    collect_until_assistant_message, extract_assistant_text, TestGateway, TestGatewayConfig,
};

#[tokio::test]
async fn session_remembers_context() {
    require_provider!(provider);

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools: Vec::new(),
        blob_store: None,
    })
    .await;

    let mut client = gw.connect().await;
    client
        .open_session(OpenConversationOpts {
            conversation_id: None,
            working_dir: None,
        })
        .await
        .expect("open session");

    // First message — introduce a fact
    client
        .send_message("My name is Alice. Just acknowledge this briefly.")
        .await
        .expect("send first message");
    let _ = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;

    // Second message — ask about the fact
    client
        .send_message("What is my name?")
        .await
        .expect("send second message");
    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;

    let text = extract_assistant_text(&frames).expect("assistant response");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("alice"),
        "response should remember 'Alice', got: {text}"
    );
}
