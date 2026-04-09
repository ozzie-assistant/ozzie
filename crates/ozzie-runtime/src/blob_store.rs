use std::path::{Path, PathBuf};

use ozzie_core::domain::{BlobError, BlobStore};
use ozzie_types::BlobRef;
use tracing::debug;

/// Filesystem-backed blob store.
///
/// Blobs are stored as `{root}/blobs/{hash}.{ext}` where ext is derived
/// from the media type (e.g. `png`, `jpeg`, `webp`).
pub struct FsBlobStore {
    blobs_dir: PathBuf,
}

impl FsBlobStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            blobs_dir: root.as_ref().join("blobs"),
        }
    }

    fn blob_path(&self, blob: &BlobRef) -> PathBuf {
        let ext = media_type_to_ext(&blob.media_type);
        self.blobs_dir.join(format!("{}.{ext}", blob.hash))
    }
}

#[async_trait::async_trait]
impl BlobStore for FsBlobStore {
    async fn write(&self, bytes: &[u8], media_type: &str) -> Result<BlobRef, BlobError> {
        validate_media_type(media_type)?;

        let hash = sha256_hex(bytes);
        let blob = BlobRef {
            hash,
            media_type: media_type.to_string(),
        };

        let path = self.blob_path(&blob);
        if path.exists() {
            debug!(hash = %blob.hash, "blob already exists, deduplicating");
            return Ok(blob);
        }

        tokio::fs::create_dir_all(&self.blobs_dir)
            .await
            .map_err(|e| BlobError::Io(e.to_string()))?;

        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| BlobError::Io(e.to_string()))?;

        debug!(hash = %blob.hash, media_type, size = bytes.len(), "blob written");
        Ok(blob)
    }

    async fn read(&self, blob: &BlobRef) -> Result<Vec<u8>, BlobError> {
        let path = self.blob_path(blob);
        tokio::fs::read(&path)
            .await
            .map_err(|_| BlobError::NotFound(blob.hash.clone()))
    }

    async fn exists(&self, blob: &BlobRef) -> bool {
        self.blob_path(blob).exists()
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256(data);
    hex::encode(digest)
}

/// Pure-Rust SHA-256 — avoids adding a crypto dependency for a single use.
fn sha256(data: &[u8]) -> [u8; 32] {
    // Minimal SHA-256 implementation — constants and algorithm
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Pre-processing: pad message
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit block
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[4 * i..4 * i + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

const SUPPORTED_MEDIA_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
];

fn validate_media_type(media_type: &str) -> Result<(), BlobError> {
    if SUPPORTED_MEDIA_TYPES.contains(&media_type) {
        Ok(())
    } else {
        Err(BlobError::UnsupportedMediaType(media_type.to_string()))
    }
}

fn media_type_to_ext(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

/// Resolves all `ContentPart::Image` blob references to `ContentPart::ImageInline`
/// with base64-encoded data. Text parts pass through unchanged.
pub async fn resolve_blobs(
    messages: &[ozzie_llm::ChatMessage],
    store: &dyn BlobStore,
) -> Result<Vec<ozzie_llm::ChatMessage>, BlobError> {
    use base64::Engine;
    let encoder = base64::engine::general_purpose::STANDARD;

    let mut resolved = Vec::with_capacity(messages.len());

    for msg in messages {
        let has_blobs = msg.content.iter().any(|p| matches!(p, ozzie_types::ContentPart::Image { .. }));

        if !has_blobs {
            resolved.push(msg.clone());
            continue;
        }

        let mut parts = Vec::with_capacity(msg.content.len());
        for part in &msg.content {
            match part {
                ozzie_types::ContentPart::Image { blob, alt } => {
                    let bytes = store.read(blob).await?;
                    let base64_data = encoder.encode(&bytes);
                    parts.push(ozzie_types::ContentPart::ImageInline {
                        media_type: blob.media_type.clone(),
                        data: base64_data,
                        alt: alt.clone(),
                    });
                }
                other => parts.push(other.clone()),
            }
        }

        resolved.push(ozzie_llm::ChatMessage {
            role: msg.role,
            content: parts,
            tool_calls: msg.tool_calls.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let hash = sha256_hex(b"hello");
        assert_eq!(hash, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn sha256_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn media_type_validation() {
        assert!(validate_media_type("image/png").is_ok());
        assert!(validate_media_type("image/jpeg").is_ok());
        assert!(validate_media_type("application/pdf").is_err());
    }

    #[test]
    fn ext_mapping() {
        assert_eq!(media_type_to_ext("image/png"), "png");
        assert_eq!(media_type_to_ext("image/jpeg"), "jpg");
        assert_eq!(media_type_to_ext("unknown/type"), "bin");
    }

    #[tokio::test]
    async fn write_and_read_blob() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let data = b"fake image data";
        let blob = store.write(data, "image/png").await.unwrap();

        assert!(!blob.hash.is_empty());
        assert_eq!(blob.media_type, "image/png");
        assert!(store.exists(&blob).await);

        let read_back = store.read(&blob).await.unwrap();
        assert_eq!(read_back, data);
    }

    #[tokio::test]
    async fn deduplication() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let data = b"same content";
        let blob1 = store.write(data, "image/png").await.unwrap();
        let blob2 = store.write(data, "image/png").await.unwrap();

        assert_eq!(blob1.hash, blob2.hash);
    }

    #[tokio::test]
    async fn unsupported_media_type() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let result = store.write(b"data", "video/mp4").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_blobs_with_images() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let data = b"fake png bytes";
        let blob = store.write(data, "image/png").await.unwrap();

        let messages = vec![ozzie_llm::ChatMessage {
            role: ozzie_llm::ChatRole::User,
            content: vec![
                ozzie_types::ContentPart::text("Look at this image:"),
                ozzie_types::ContentPart::image(blob),
            ],
            tool_calls: Vec::new(),
            tool_call_id: None,
        }];

        let resolved = resolve_blobs(&messages, &store).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].content.len(), 2);

        match &resolved[0].content[1] {
            ozzie_types::ContentPart::ImageInline { media_type, data, .. } => {
                assert_eq!(media_type, "image/png");
                assert!(!data.is_empty());
            }
            other => panic!("expected ImageInline, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_blobs_text_only_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let messages = vec![ozzie_llm::ChatMessage::text(ozzie_llm::ChatRole::User, "hello")];
        let resolved = resolve_blobs(&messages, &store).await.unwrap();

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].text_content(), "hello");
    }
}
