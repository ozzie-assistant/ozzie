use std::collections::HashMap;
use std::sync::Arc;

use ozzie_memory::{MemoryEntry, MemoryError, Store};

/// In-memory store for testing memory tools.
pub struct FakeStore {
    entries: tokio::sync::Mutex<HashMap<String, (MemoryEntry, String)>>,
}

impl FakeStore {
    pub fn new_arc() -> Arc<dyn Store> {
        Arc::new(Self {
            entries: tokio::sync::Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait::async_trait]
impl Store for FakeStore {
    async fn create(&self, entry: &mut MemoryEntry, content: &str) -> Result<(), MemoryError> {
        entry.id = format!("mem_{}", self.entries.lock().await.len() + 1);
        self.entries
            .lock()
            .await
            .insert(entry.id.clone(), (entry.clone(), content.to_string()));
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<(MemoryEntry, String), MemoryError> {
        self.entries
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))
    }

    async fn update(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError> {
        self.entries
            .lock()
            .await
            .insert(entry.id.clone(), (entry.clone(), content.to_string()));
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        self.entries
            .lock()
            .await
            .remove(id)
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(self
            .entries
            .lock()
            .await
            .values()
            .map(|(e, _)| e.clone())
            .collect())
    }
}
