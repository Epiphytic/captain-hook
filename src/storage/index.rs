use std::fs;
use std::path::PathBuf;

use crate::error::{CaptainHookError, Result};

/// Wrapper around the HNSW index for persistent storage.
pub struct HnswIndexStore {
    index_dir: PathBuf,
}

impl HnswIndexStore {
    pub fn new(index_dir: PathBuf) -> Self {
        Self { index_dir }
    }

    /// Validate that an index name doesn't contain path traversal characters.
    fn validate_name(name: &str) -> Result<()> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(CaptainHookError::Storage {
                reason: format!(
                    "invalid index name '{}': must not contain '/', '\\', or '..'",
                    name
                ),
            });
        }
        Ok(())
    }

    /// Save an index to disk.
    pub fn save(&self, name: &str, data: &[u8]) -> Result<()> {
        Self::validate_name(name)?;
        fs::create_dir_all(&self.index_dir)?;
        let path = self.index_dir.join(name);
        fs::write(&path, data).map_err(|e| CaptainHookError::Storage {
            reason: format!("failed to write index {}: {}", path.display(), e),
        })
    }

    /// Load an index from disk.
    pub fn load(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Self::validate_name(name)?;
        let path = self.index_dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path).map_err(|e| CaptainHookError::Storage {
            reason: format!("failed to read index {}: {}", path.display(), e),
        })?;
        Ok(Some(data))
    }

    /// Check if an index exists.
    pub fn exists(&self, name: &str) -> bool {
        Self::validate_name(name).is_ok() && self.index_dir.join(name).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let store = HnswIndexStore::new(tmp.path().to_path_buf());

        let data = b"test index data";
        store.save("test.idx", data).unwrap();

        assert!(store.exists("test.idx"));

        let loaded = store.load("test.idx").unwrap();
        assert_eq!(loaded.unwrap(), data);
    }

    #[test]
    fn test_load_missing() {
        let tmp = TempDir::new().unwrap();
        let store = HnswIndexStore::new(tmp.path().to_path_buf());

        assert!(!store.exists("missing.idx"));
        let loaded = store.load("missing.idx").unwrap();
        assert!(loaded.is_none());
    }
}
