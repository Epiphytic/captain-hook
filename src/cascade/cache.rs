use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier};
use crate::error::Result;

/// Tier 1: Exact cache lookup.
pub struct ExactCache {
    entries: RwLock<HashMap<CacheKey, DecisionRecord>>,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
}

impl Default for ExactCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ExactCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Load cache from stored decisions.
    pub fn load_from(&self, records: Vec<DecisionRecord>) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        for record in records {
            entries.insert(record.key.clone(), record);
        }
    }

    /// Insert or update a cache entry.
    pub fn insert(&self, record: DecisionRecord) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.insert(record.key.clone(), record);
    }

    /// Remove all entries for a specific role.
    pub fn invalidate_role(&self, role: &str) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.retain(|k, _| k.role != role);
    }

    /// Remove all entries.
    pub fn invalidate_all(&self) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.clear();
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let mut stats = CacheStats {
            total_entries: entries.len(),
            hits: self.hits.load(std::sync::atomic::Ordering::Relaxed),
            misses: self.misses.load(std::sync::atomic::Ordering::Relaxed),
            ..Default::default()
        };
        for record in entries.values() {
            match record.decision {
                Decision::Allow => stats.allow_entries += 1,
                Decision::Deny => stats.deny_entries += 1,
                Decision::Ask => stats.ask_entries += 1,
            }
        }
        stats
    }
}

#[async_trait]
impl CascadeTier for ExactCache {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>> {
        let role_name = input
            .session
            .role
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "*".to_string());

        let key = CacheKey {
            sanitized_input: input.sanitized_input.clone(),
            tool: input.tool_name.clone(),
            role: role_name.clone(),
        };

        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());

        // Try exact role match first, then wildcard
        let record = entries.get(&key).or_else(|| {
            let wildcard_key = CacheKey {
                sanitized_input: input.sanitized_input.clone(),
                tool: input.tool_name.clone(),
                role: "*".to_string(),
            };
            entries.get(&wildcard_key)
        });

        match record {
            Some(cached) => {
                self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                // Return a new record with ExactCache tier metadata
                Ok(Some(DecisionRecord {
                    key: cached.key.clone(),
                    decision: cached.decision,
                    metadata: DecisionMetadata {
                        tier: DecisionTier::ExactCache,
                        confidence: 1.0,
                        reason: format!(
                            "exact cache hit: {} (originally from {:?})",
                            cached.decision, cached.metadata.tier
                        ),
                        matched_key: Some(cached.key.clone()),
                        similarity_score: None,
                    },
                    timestamp: Utc::now(),
                    scope: cached.scope,
                    file_path: cached.file_path.clone(),
                    session_id: String::new(), // Filled by CascadeRunner
                }))
            }
            None => {
                self.misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    fn tier(&self) -> DecisionTier {
        DecisionTier::ExactCache
    }

    fn name(&self) -> &str {
        "exact-cache"
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_entries: usize,
    pub allow_entries: usize,
    pub deny_entries: usize,
    pub ask_entries: usize,
    pub hits: u64,
    pub misses: u64,
}
