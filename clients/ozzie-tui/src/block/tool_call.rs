use chrono::{DateTime, Utc};

use super::{BlockId, BlockState};

/// A tool call block — collapsible in the viewport.
#[derive(Debug, Clone)]
pub struct ToolCallBlock {
    pub id: BlockId,
    pub ts: DateTime<Utc>,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub is_error: bool,
    pub collapsed: bool,
    pub state: BlockState,
}

impl ToolCallBlock {
    /// Toggles collapsed/expanded state.
    pub fn toggle_collapse(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// Marks the tool call as finalized (completed).
    pub fn finalize(&mut self) {
        self.state = BlockState::Finalized;
    }

    /// Sets the tool result and finalizes the block.
    pub fn set_result(&mut self, result: String, is_error: bool) {
        self.result = Some(result);
        self.is_error = is_error;
        self.finalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_block() -> ToolCallBlock {
        ToolCallBlock {
            id: 1,
            ts: Utc::now(),
            call_id: "tc_1".to_string(),
            name: "file_read".to_string(),
            arguments: r#"{"path":"/tmp/test.rs"}"#.to_string(),
            result: None,
            is_error: false,
            collapsed: true,
            state: BlockState::Active,
        }
    }

    #[test]
    fn toggle_and_finalize() {
        let mut block = new_block();

        assert!(block.collapsed);
        block.toggle_collapse();
        assert!(!block.collapsed);
        block.toggle_collapse();
        assert!(block.collapsed);

        block.finalize();
        assert_eq!(block.state, BlockState::Finalized);
    }

    #[test]
    fn set_result_finalizes() {
        let mut block = new_block();
        block.set_result("ok".to_string(), false);
        assert_eq!(block.state, BlockState::Finalized);
        assert_eq!(block.result.as_deref(), Some("ok"));
        assert!(!block.is_error);
    }

    #[test]
    fn set_result_error() {
        let mut block = new_block();
        block.set_result("Error: not found".to_string(), true);
        assert!(block.is_error);
        assert_eq!(block.state, BlockState::Finalized);
    }
}
