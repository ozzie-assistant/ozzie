//! File-backed typed configuration storage.
//!
//! [`FileStorage`] implements [`ConfigStore`] from `ozzie-core` using
//! a JSON file with read-write locking.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::de::DeserializeOwned;
use serde::Serialize;

use ozzie_core::storage::{ConfigStore, StorageError};

/// File-backed JSON store with read-write locking.
///
/// - [`read`](ConfigStore::read) returns `T::default()` if the file does not exist yet.
/// - [`patch`](ConfigStore::patch) and [`save`](ConfigStore::save) create parent directories as needed.
pub struct FileStorage<T> {
    path: PathBuf,
    lock: RwLock<()>,
    _phantom: PhantomData<T>,
}

impl<T> FileStorage<T> {
    /// Creates a store backed by `path`.
    ///
    /// The file is not created until the first write.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            lock: RwLock::new(()),
            _phantom: PhantomData,
        }
    }

    /// Returns the backing file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<T> ConfigStore<T> for FileStorage<T>
where
    T: DeserializeOwned + Serialize + Default + Send + Sync,
{
    fn read(&self) -> Result<T, StorageError> {
        let _guard = self.lock.read().map_err(|_| StorageError::Poisoned)?;
        if !self.path.exists() {
            return Ok(T::default());
        }
        let content = std::fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn patch(&self, f: Box<dyn FnOnce(T) -> T + Send>) -> Result<T, StorageError> {
        let _guard = self.lock.write().map_err(|_| StorageError::Poisoned)?;
        let current = if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)?;
            serde_json::from_str::<T>(&content).unwrap_or_default()
        } else {
            T::default()
        };
        let updated = f(current);
        write_json(&self.path, &updated)?;
        Ok(updated)
    }

    fn save(&self, value: T) -> Result<(), StorageError> {
        let _guard = self.lock.write().map_err(|_| StorageError::Poisoned)?;
        write_json(&self.path, &value)
    }
}

/// Serializes `value` as pretty JSON and writes it to `path`.
///
/// Creates parent directories if needed. Does not hold any lock — callers are
/// responsible for holding the write guard before calling.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Sample {
        count: u32,
        label: String,
    }

    #[test]
    fn read_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStorage::<Sample>::new(dir.path().join("data.json"));
        let val = store.read().unwrap();
        assert_eq!(val, Sample::default());
    }

    #[test]
    fn save_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStorage::<Sample>::new(dir.path().join("data.json"));
        let original = Sample { count: 42, label: "hello".to_string() };
        store.save(original).unwrap();
        let read_back = store.read().unwrap();
        assert_eq!(read_back, Sample { count: 42, label: "hello".to_string() });
    }

    #[test]
    fn patch_modifies_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStorage::<Sample>::new(dir.path().join("data.json"));
        store.save(Sample { count: 1, label: "x".to_string() }).unwrap();
        let updated = store.patch(Box::new(|mut s| {
            s.count += 10;
            s
        })).unwrap();
        assert_eq!(updated.count, 11);
        assert_eq!(store.read().unwrap().count, 11);
    }

    #[test]
    fn patch_on_missing_uses_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStorage::<Sample>::new(dir.path().join("data.json"));
        let updated = store.patch(Box::new(|mut s| {
            s.count = 99;
            s
        })).unwrap();
        assert_eq!(updated.count, 99);
    }

    #[test]
    fn creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStorage::<Sample>::new(dir.path().join("a/b/c/data.json"));
        store.save(Sample::default()).unwrap();
        assert!(dir.path().join("a/b/c/data.json").exists());
    }
}
