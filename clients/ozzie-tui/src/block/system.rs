use chrono::{DateTime, Utc};

use super::BlockId;

/// A system notification block — always finalized on creation.
#[derive(Debug, Clone)]
pub struct SystemBlock {
    pub id: BlockId,
    pub ts: DateTime<Utc>,
    pub content: String,
}
