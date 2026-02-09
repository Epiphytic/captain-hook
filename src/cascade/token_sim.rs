use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier};
use crate::error::Result;

/// A token set entry for Jaccard comparison.
#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub tokens: Vec<String>,
    pub cache_key: CacheKey,
    pub record: DecisionRecord,
}

/// Tier 2a: Token-level Jaccard similarity.
pub struct TokenJaccard {
    entries: RwLock<Vec<TokenEntry>>,
    threshold: f64,
    min_tokens: usize,
}

impl TokenJaccard {
    pub fn new(threshold: f64, min_tokens: usize) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            threshold,
            min_tokens,
        }
    }

    /// Load entries from cached decisions.
    pub fn load_from(&self, records: &[DecisionRecord]) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        for record in records {
            let tokens = Self::tokenize(&record.key.sanitized_input);
            entries.push(TokenEntry {
                tokens,
                cache_key: record.key.clone(),
                record: record.clone(),
            });
        }
    }

    /// Add a single entry.
    pub fn insert(&self, record: &DecisionRecord) {
        let tokens = Self::tokenize(&record.key.sanitized_input);
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.push(TokenEntry {
            tokens,
            cache_key: record.key.clone(),
            record: record.clone(),
        });
    }

    /// Tokenize an input string: split on whitespace + punctuation, lowercase,
    /// deduplicate, sort.
    pub fn tokenize(input: &str) -> Vec<String> {
        let mut tokens: Vec<String> = input
            .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        tokens.sort();
        tokens.dedup();
        tokens
    }

    /// Compute Jaccard coefficient between two sorted token slices.
    pub fn jaccard_coefficient(a: &[String], b: &[String]) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        let intersection = Self::sorted_intersection_count(a, b);
        let union = a.len() + b.len() - intersection;
        if union == 0 {
            return 0.0;
        }
        intersection as f64 / union as f64
    }

    /// Count intersection of two sorted slices using merge-join.
    fn sorted_intersection_count(a: &[String], b: &[String]) -> usize {
        let mut count = 0;
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    count += 1;
                    i += 1;
                    j += 1;
                }
            }
        }
        count
    }

    /// Remove all entries for a specific role.
    pub fn invalidate_role(&self, role: &str) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.retain(|e| e.cache_key.role != role);
    }

    /// Remove all entries.
    pub fn invalidate_all(&self) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.clear();
    }
}

#[async_trait]
impl CascadeTier for TokenJaccard {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>> {
        let query_tokens = Self::tokenize(&input.sanitized_input);

        // Skip if too few tokens
        if query_tokens.len() < self.min_tokens {
            return Ok(None);
        }

        let role_name = input
            .session
            .role
            .as_ref()
            .map(|r| r.name.as_str())
            .unwrap_or("*");

        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());

        let mut best_match: Option<(f64, &TokenEntry)> = None;

        for entry in entries.iter() {
            // Only match same role or wildcard entries
            if entry.cache_key.role != role_name && entry.cache_key.role != "*" {
                continue;
            }
            // Only match same tool
            if entry.cache_key.tool != input.tool_name {
                continue;
            }

            let score = Self::jaccard_coefficient(&query_tokens, &entry.tokens);

            if score >= self.threshold && best_match.as_ref().is_none_or(|(best, _)| score > *best)
            {
                best_match = Some((score, entry));
            }
        }

        match best_match {
            Some((score, entry)) => {
                // Similarity behavior:
                // - allow -> auto-approve
                // - deny -> fall through (similarity never auto-denies)
                // - ask -> return ask (escalate)
                match entry.record.decision {
                    Decision::Deny => Ok(None), // Never auto-deny from similarity
                    Decision::Allow | Decision::Ask => {
                        Ok(Some(DecisionRecord {
                            key: CacheKey {
                                sanitized_input: input.sanitized_input.clone(),
                                tool: input.tool_name.clone(),
                                role: role_name.to_string(),
                            },
                            decision: entry.record.decision,
                            metadata: DecisionMetadata {
                                tier: DecisionTier::TokenJaccard,
                                confidence: score,
                                reason: format!(
                                    "token Jaccard similarity {:.3} >= {:.3} with cached {}",
                                    score, self.threshold, entry.record.decision
                                ),
                                matched_key: Some(entry.cache_key.clone()),
                                similarity_score: Some(score),
                            },
                            timestamp: Utc::now(),
                            scope: entry.record.scope,
                            file_path: input.file_path.clone(),
                            session_id: String::new(), // Filled by CascadeRunner
                        }))
                    }
                }
            }
            None => Ok(None), // No match above threshold
        }
    }

    fn tier(&self) -> DecisionTier {
        DecisionTier::TokenJaccard
    }

    fn name(&self) -> &str {
        "token-jaccard"
    }
}
