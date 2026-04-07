use chrono::{DateTime, Utc};

use super::{BlockId, BlockState};

/// An assistant response block that supports streaming.
#[derive(Debug, Clone)]
pub struct AssistantBlock {
    pub id: BlockId,
    pub ts: DateTime<Utc>,
    pub content: String,
    pub state: BlockState,
}

impl AssistantBlock {
    /// Appends a streaming delta to the content.
    pub fn append_delta(&mut self, delta: &str) {
        self.content.push_str(delta);
    }

    /// Marks the block as finalized (streaming complete).
    pub fn finalize(&mut self) {
        self.state = BlockState::Finalized;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_lifecycle() {
        let mut block = AssistantBlock {
            id: 1,
            ts: Utc::now(),
            content: String::new(),
            state: BlockState::Active,
        };

        block.append_delta("Hello");
        block.append_delta(" world");
        assert_eq!(block.content, "Hello world");
        assert_eq!(block.state, BlockState::Active);

        block.finalize();
        assert_eq!(block.state, BlockState::Finalized);
    }
}
