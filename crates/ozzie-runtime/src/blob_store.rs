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
    dir_created: std::sync::Once,
}

impl FsBlobStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            blobs_dir: root.as_ref().join("blobs"),
            dir_created: std::sync::Once::new(),
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
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            debug!(hash = %blob.hash, "blob already exists, deduplicating");
            return Ok(blob);
        }

        // Ensure blobs dir exists (once per lifetime).
        let dir = self.blobs_dir.clone();
        self.dir_created.call_once(|| {
            let _ = std::fs::create_dir_all(&dir);
        });

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
        tokio::fs::try_exists(self.blob_path(blob))
            .await
            .unwrap_or(false)
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
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

/// Resolves a single `BlobRef` to an `ozzie_llm::Content::Image` with base64 data.
pub async fn resolve_blob_to_content(
    blob: &BlobRef,
    store: &dyn BlobStore,
) -> Result<ozzie_llm::Content, BlobError> {
    use base64::Engine;
    let encoder = base64::engine::general_purpose::STANDARD;
    let bytes = store.read(blob).await?;
    let base64_data = encoder.encode(&bytes);
    Ok(ozzie_llm::Content::Image {
        media_type: blob.media_type.clone(),
        data: base64_data,
        alt: None,
    })
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
    async fn resolve_blob_to_content_produces_image() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path());

        let data = b"fake png bytes";
        let blob = store.write(data, "image/png").await.unwrap();

        let content = resolve_blob_to_content(&blob, &store).await.unwrap();
        match &content {
            ozzie_llm::Content::Image { media_type, data, .. } => {
                assert_eq!(media_type, "image/png");
                assert!(!data.is_empty());
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }
}
