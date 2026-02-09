pub mod index;
pub mod jsonl;

use std::path::Path;

use crate::decision::DecisionRecord;
use crate::error::Result;
use crate::scope::ScopeLevel;

/// Backend for loading and saving decision records.
pub trait StorageBackend: Send + Sync {
    /// Load all decisions from storage for a given scope.
    fn load_decisions(&self, scope: ScopeLevel) -> Result<Vec<DecisionRecord>>;

    /// Load decisions filtered by role.
    fn load_decisions_for_role(&self, scope: ScopeLevel, role: &str)
        -> Result<Vec<DecisionRecord>>;

    /// Save a single decision.
    fn save_decision(&self, record: &DecisionRecord) -> Result<()>;

    /// Delete all decisions for a specific role within a scope.
    fn invalidate_role(&self, scope: ScopeLevel, role: &str) -> Result<()>;

    /// Delete all decisions within a scope.
    fn invalidate_all(&self, scope: ScopeLevel) -> Result<()>;

    /// Rebuild the HNSW index from stored decisions.
    fn rebuild_index(&self, scope: ScopeLevel) -> Result<()>;

    /// Scan stored decisions for secrets that may have bypassed sanitization.
    fn scan_for_secrets(&self, path: &Path) -> Result<Vec<SecretFinding>>;
}

/// A potential secret found during scanning.
#[derive(Debug, Clone)]
pub struct SecretFinding {
    pub file: std::path::PathBuf,
    pub line: usize,
    pub description: String,
    pub detector: String,
}
