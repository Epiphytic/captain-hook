pub mod hierarchy;
pub mod merge;

use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::decision::{CacheKey, Decision, DecisionRecord};
use crate::error::Result;
use crate::session::SessionContext;
use crate::storage::StorageBackend;

/// The four scope levels, ordered from broadest to narrowest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeLevel {
    Org,
    Project,
    User,
    Role,
}

/// A decision with its originating scope.
#[derive(Debug, Clone)]
pub struct ScopedDecision {
    /// The effective decision after scope precedence resolution.
    pub decision: Decision,
    /// Which scope the decision originated from.
    pub scope: ScopeLevel,
    /// The full record from the originating scope.
    pub record: DecisionRecord,
}

/// Resolves the effective decision across all scopes.
///
/// Precedence: DENY > ASK > ALLOW > silent
pub struct ScopeResolver {
    storage: Box<dyn StorageBackend>,
    cache: RwLock<Option<HashMap<ScopeLevel, Vec<DecisionRecord>>>>,
}

impl ScopeResolver {
    pub fn new(storage: Box<dyn StorageBackend>) -> Self {
        Self {
            storage,
            cache: RwLock::new(None),
        }
    }

    /// Populate the in-memory cache from storage. Called lazily on first resolve().
    fn ensure_cache(&self) -> Result<()> {
        {
            let guard = self.cache.read().unwrap_or_else(|e| e.into_inner());
            if guard.is_some() {
                return Ok(());
            }
        }
        let mut map = HashMap::new();
        for &scope in &[
            ScopeLevel::Role,
            ScopeLevel::User,
            ScopeLevel::Project,
            ScopeLevel::Org,
        ] {
            let decisions = self.storage.load_decisions(scope)?;
            map.insert(scope, decisions);
        }
        let mut guard = self.cache.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(map);
        Ok(())
    }

    /// Force reload the cache from disk.
    pub fn reload(&self) -> Result<()> {
        {
            let mut guard = self.cache.write().unwrap_or_else(|e| e.into_inner());
            *guard = None;
        }
        self.ensure_cache()
    }

    /// Resolve the effective decision across all scopes for a given cache key.
    ///
    /// Checks scopes in order: Role -> User -> Project -> Org.
    /// Applies precedence: DENY > ASK > ALLOW > silent.
    ///
    /// Returns None if no scope has a matching decision (novel command).
    pub fn resolve(
        &self,
        key: &CacheKey,
        _session: &SessionContext,
    ) -> Result<Option<ScopedDecision>> {
        self.ensure_cache()?;

        let scopes = [
            ScopeLevel::Role,
            ScopeLevel::User,
            ScopeLevel::Project,
            ScopeLevel::Org,
        ];

        let mut found: Vec<ScopedDecision> = Vec::new();

        let guard = self.cache.read().unwrap_or_else(|e| e.into_inner());
        let cache_map = guard.as_ref().expect("cache populated by ensure_cache");

        for &scope in &scopes {
            if let Some(decisions) = cache_map.get(&scope) {
                for record in decisions {
                    if record.key == *key
                        || (record.key.role == "*"
                            && record.key.tool == key.tool
                            && record.key.sanitized_input == key.sanitized_input)
                    {
                        found.push(ScopedDecision {
                            decision: record.decision,
                            scope,
                            record: record.clone(),
                        });
                    }
                }
            }
        }

        Ok(merge::merge_decisions(found))
    }
}
