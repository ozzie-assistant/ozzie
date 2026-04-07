use chrono::{DateTime, Utc};

use super::BlockId;

/// A user message block — always finalized on creation.
#[derive(Debug, Clone)]
pub struct UserBlock {
    pub id: BlockId,
    pub ts: DateTime<Utc>,
    pub content: String,
}
