use std::time::Duration;

use ozzie_client::OpenConversationOpts;

use crate::harness::{
    collect_until_assistant_message, extract_assistant_text, TestGateway, TestGatewayConfig,
};

#[tokio::test]
async fn simple_chat_returns_answer() {
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

    client
        .send_message("What is 2+2? Reply with just the number, nothing else.")
        .await
        .expect("send message");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;

    let text = extract_assistant_text(&frames).expect("should have assistant.message");
    assert!(!text.is_empty(), "response should not be empty");
    assert!(
        text.contains('4'),
        "response should contain '4', got: {text}"
    );
}
