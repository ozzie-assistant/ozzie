use std::sync::Arc;
use std::time::Duration;

use ozzie_client::OpenSessionOpts;
use ozzie_core::domain::MemoryStore;
use ozzie_memory::MarkdownStore;
use ozzie_tools::ToolRegistry;

use crate::harness::{
    collect_until_assistant_message, extract_assistant_text, TestGateway, TestGatewayConfig,
};

#[tokio::test]
async fn store_and_query_memory() {
    require_provider!(provider);

    let memory_dir = tempfile::tempdir().expect("create memory tempdir");
    let memory_store =
        Arc::new(MarkdownStore::new(memory_dir.path()).expect("create memory store"));

    let registry = Arc::new(ToolRegistry::new());
    ozzie_tools::native::register_memory_tools(
        &registry,
        memory_store.clone() as Arc<dyn ozzie_memory::Store>,
        None,
    );
    let tools = registry.all_tools();

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools,
        blob_store: None,
    })
    .await;

    // Conversation 1: store a memory
    let mut client = gw.connect().await;
    client
        .open_session(OpenSessionOpts {
            session_id: None,
            working_dir: None,
        })
        .await
        .expect("open session 1");

    client
        .send_message("Remember this fact: my favorite color is blue. Use the store_memory tool to save it.")
        .await
        .expect("send store request");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;
    let text = extract_assistant_text(&frames);
    assert!(text.is_some(), "should get a response after storing memory");

    // Verify the memory was actually stored
    let results = memory_store.search_text("favorite color", 5).await;
    assert!(
        results.is_ok() && !results.unwrap().is_empty(),
        "memory store should contain the stored fact"
    );

    // Conversation 2: query the memory
    let mut client2 = gw.connect().await;
    client2
        .open_session(OpenSessionOpts {
            session_id: None,
            working_dir: None,
        })
        .await
        .expect("open session 2");

    client2
        .send_message("What is my favorite color? Use query_memories to find out.")
        .await
        .expect("send query request");

    let frames2 = collect_until_assistant_message(&mut client2, Duration::from_secs(120)).await;
    let text2 = extract_assistant_text(&frames2).expect("should get response");
    let lower = text2.to_lowercase();
    assert!(
        lower.contains("blue"),
        "response should mention 'blue', got: {text2}"
    );
}
