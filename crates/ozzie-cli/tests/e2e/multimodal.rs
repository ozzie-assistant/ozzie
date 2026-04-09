use std::sync::Arc;
use std::time::Duration;

use ozzie_client::OpenSessionOpts;
use ozzie_runtime::FsBlobStore;
use ozzie_types::ImageAttachment;

use crate::harness::{
    collect_until_assistant_message, extract_assistant_text, TestGateway, TestGatewayConfig,
};

static TEST_IMAGE: &[u8] = include_bytes!("../ressources/image.png");

#[tokio::test]
async fn describe_image() {
    require_vision_provider!(provider);

    let blob_dir = tempfile::tempdir().expect("create blob tempdir");
    let blob_store = Arc::new(FsBlobStore::new(blob_dir.path()));

    let gw = TestGateway::start(TestGatewayConfig {
        provider,
        tools: Vec::new(),
        blob_store: Some(blob_store as Arc<dyn ozzie_core::domain::BlobStore>),
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

    // Encode test image as base64
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(TEST_IMAGE);

    client
        .send_message_with_images(
            "Describe this image in one sentence.",
            vec![ImageAttachment {
                data: b64,
                media_type: "image/png".into(),
                alt: None,
            }],
        )
        .await
        .expect("send image message");

    let frames = collect_until_assistant_message(&mut client, Duration::from_secs(120)).await;

    let text = extract_assistant_text(&frames).expect("should get assistant response");
    assert!(!text.is_empty(), "response should not be empty");
}
