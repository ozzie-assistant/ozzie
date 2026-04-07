mod assistant;
mod system;
mod tool_call;
mod user;

pub use assistant::AssistantBlock;
pub use system::SystemBlock;
pub use tool_call::ToolCallBlock;
pub use user::UserBlock;

/// Unique identifier for a block.
pub type BlockId = u64;

/// Whether a block is still being updated or is finalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    Active,
    Finalized,
}

/// A content block in the conversation stream.
#[derive(Debug, Clone)]
pub enum Block {
    User(UserBlock),
    Assistant(AssistantBlock),
    ToolCall(ToolCallBlock),
    System(SystemBlock),
}

impl Block {
    pub fn id(&self) -> BlockId {
        match self {
            Self::User(b) => b.id,
            Self::Assistant(b) => b.id,
            Self::ToolCall(b) => b.id,
            Self::System(b) => b.id,
        }
    }

    pub fn ts(&self) -> chrono::DateTime<chrono::Utc> {
        match self {
            Self::User(b) => b.ts,
            Self::Assistant(b) => b.ts,
            Self::ToolCall(b) => b.ts,
            Self::System(b) => b.ts,
        }
    }

    pub fn state(&self) -> BlockState {
        match self {
            Self::User(_) | Self::System(_) => BlockState::Finalized,
            Self::Assistant(b) => b.state,
            Self::ToolCall(b) => b.state,
        }
    }

    pub fn is_finalized(&self) -> bool {
        self.state() == BlockState::Finalized
    }
}
