use serde::{Deserialize, Serialize};

/// A content part in a chat message — text or inline image.
///
/// This enum represents content as consumed by LLM providers.
/// Image data must be pre-resolved to base64 before building a `ChatMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
}

impl Content {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    pub fn image(media_type: impl Into<String>, base64_data: impl Into<String>) -> Self {
        Self::Image {
            media_type: media_type.into(),
            data: base64_data.into(),
            alt: None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }
}

/// Collapse `Vec<Content>` into a single text string (ignoring non-text parts).
pub fn parts_to_text(parts: &[Content]) -> String {
    let texts: Vec<&str> = parts.iter().filter_map(|p| p.as_text()).collect();
    texts.join("\n")
}

/// Wrap a text string into a single-element content vec.
pub fn text_to_parts(text: impl Into<String>) -> Vec<Content> {
    vec![Content::text(text)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_roundtrip() {
        let parts = text_to_parts("hello");
        assert_eq!(parts_to_text(&parts), "hello");
    }

    #[test]
    fn mixed_content_text_only() {
        let parts = vec![
            Content::text("before"),
            Content::image("image/png", "base64data"),
            Content::text("after"),
        ];
        assert_eq!(parts_to_text(&parts), "before\nafter");
    }

    #[test]
    fn serde_roundtrip() {
        let parts = vec![
            Content::text("hello"),
            Content::image("image/jpeg", "abc123"),
        ];
        let json = serde_json::to_string(&parts).unwrap();
        let back: Vec<Content> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back[0].as_text().is_some());
        assert!(back[1].is_image());
    }
}
