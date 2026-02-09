use std::sync::{Mutex, RwLock};

use async_trait::async_trait;
use chrono::Utc;

use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier};
use crate::error::{CaptainHookError, Result};

/// An entry in the HNSW index.
#[derive(Debug, Clone)]
pub struct EmbeddingEntry {
    pub embedding: Vec<f32>,
    pub record: DecisionRecord,
}

/// A point in the embedding space (wrapper for instant-distance).
#[derive(Clone)]
pub struct Point(pub Vec<f32>);

impl instant_distance::Point for Point {
    fn distance(&self, other: &Self) -> f32 {
        // Cosine distance = 1 - cosine_similarity
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        let norm_a: f32 = self.0.iter().map(|a| a * a).sum::<f32>().sqrt();
        let norm_b: f32 = other.0.iter().map(|b| b * b).sum::<f32>().sqrt();
        let denom = norm_a * norm_b;
        if denom == 0.0 {
            return 1.0;
        }
        1.0 - (dot / denom)
    }
}

/// Wrapper around instant-distance HNSW index.
pub struct HnswIndex {
    hnsw: instant_distance::HnswMap<Point, usize>,
}

/// Maximum pending entries before an automatic rebuild.
const PENDING_REBUILD_THRESHOLD: usize = 50;

/// Tier 2b: Embedding-based HNSW similarity search.
pub struct EmbeddingSimilarity {
    index: RwLock<Option<HnswIndex>>,
    model: Option<Mutex<fastembed::TextEmbedding>>,
    threshold: f64,
    entries: RwLock<Vec<EmbeddingEntry>>,
    /// Buffer for entries not yet in the HNSW index (linear-scanned on search).
    pending_entries: RwLock<Vec<EmbeddingEntry>>,
}

impl EmbeddingSimilarity {
    /// Create a new embedding similarity engine.
    pub fn new(_model_name: &str, threshold: f64) -> Result<Self> {
        let model = fastembed::TextEmbedding::try_new(Default::default()).map_err(|e| {
            CaptainHookError::Embedding {
                reason: e.to_string(),
            }
        })?;
        Ok(Self {
            index: RwLock::new(None),
            model: Some(Mutex::new(model)),
            threshold,
            entries: RwLock::new(Vec::new()),
            pending_entries: RwLock::new(Vec::new()),
        })
    }

    /// Create a no-op embedding tier that always returns None.
    /// Used when the embedding model is unavailable.
    pub fn new_noop() -> Self {
        Self {
            index: RwLock::new(None),
            model: None,
            threshold: f64::MAX,
            entries: RwLock::new(Vec::new()),
            pending_entries: RwLock::new(Vec::new()),
        }
    }

    /// Build/rebuild the HNSW index from a set of decision records.
    pub fn build_index(&self, records: &[DecisionRecord]) -> Result<()> {
        if records.is_empty() {
            let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
            *index = None;
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.clear();
            return Ok(());
        }

        // Generate embeddings for all records
        let texts: Vec<&str> = records
            .iter()
            .map(|r| r.key.sanitized_input.as_str())
            .collect();
        let model_mutex = self
            .model
            .as_ref()
            .ok_or_else(|| CaptainHookError::Embedding {
                reason: "embedding model not available (noop tier)".into(),
            })?;
        let embeddings = {
            let mut model = model_mutex.lock().unwrap_or_else(|e| e.into_inner());
            model
                .embed(texts, None)
                .map_err(|e| CaptainHookError::Embedding {
                    reason: e.to_string(),
                })?
        };

        // Build entries
        let mut new_entries = Vec::with_capacity(records.len());
        for (i, (record, embedding)) in records.iter().zip(embeddings.iter()).enumerate() {
            new_entries.push(EmbeddingEntry {
                embedding: embedding.clone(),
                record: record.clone(),
            });
            let _ = i; // index position matches
        }

        // Build HNSW index
        let points: Vec<Point> = embeddings.iter().map(|e| Point(e.clone())).collect();
        let values: Vec<usize> = (0..points.len()).collect();
        let hnsw = instant_distance::Builder::default().build(points, values);

        {
            let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
            *idx = Some(HnswIndex { hnsw });
        }
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            *entries = new_entries;
        }

        Ok(())
    }

    /// Add a single entry to the pending buffer. Does NOT rebuild the HNSW index.
    /// Pending entries are searched via linear scan until `rebuild()` is called
    /// or the pending buffer exceeds the threshold.
    pub fn insert(&self, record: &DecisionRecord) -> Result<()> {
        let embedding = self.embed(&record.key.sanitized_input)?;

        let should_rebuild = {
            let mut pending = self
                .pending_entries
                .write()
                .unwrap_or_else(|e| e.into_inner());
            pending.push(EmbeddingEntry {
                embedding,
                record: record.clone(),
            });
            pending.len() >= PENDING_REBUILD_THRESHOLD
        };

        if should_rebuild {
            self.rebuild()?;
        }

        Ok(())
    }

    /// Flush pending entries into the main entries list and rebuild the HNSW index.
    pub fn rebuild(&self) -> Result<()> {
        // Move pending entries into the main entries list
        let drained: Vec<EmbeddingEntry> = {
            let mut pending = self
                .pending_entries
                .write()
                .unwrap_or_else(|e| e.into_inner());
            pending.drain(..).collect()
        };

        if drained.is_empty() {
            return Ok(());
        }

        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.extend(drained);
        }

        // Rebuild HNSW from all entries
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let points: Vec<Point> = entries.iter().map(|e| Point(e.embedding.clone())).collect();
        let values: Vec<usize> = (0..points.len()).collect();

        if !points.is_empty() {
            let hnsw = instant_distance::Builder::default().build(points, values);
            let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
            *idx = Some(HnswIndex { hnsw });
        }

        Ok(())
    }

    /// Generate an embedding for a text input.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let model_mutex = self
            .model
            .as_ref()
            .ok_or_else(|| CaptainHookError::Embedding {
                reason: "embedding model not available (noop tier)".into(),
            })?;
        let mut model = model_mutex.lock().unwrap_or_else(|e| e.into_inner());
        let embeddings =
            model
                .embed(vec![text], None)
                .map_err(|e| CaptainHookError::Embedding {
                    reason: e.to_string(),
                })?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| CaptainHookError::Embedding {
                reason: "no embedding returned".into(),
            })
    }

    /// Search the index for the nearest neighbor.
    /// Checks both the HNSW index and the pending entries buffer.
    /// Returns the best match above the threshold, or None.
    pub fn search(&self, query_embedding: &[f32]) -> Option<(f64, EmbeddingEntry)> {
        let mut best: Option<(f64, EmbeddingEntry)> = None;

        // 1. Search the HNSW index
        {
            let index_guard = self.index.read().unwrap_or_else(|e| e.into_inner());
            if let Some(hnsw_index) = index_guard.as_ref() {
                let query_point = Point(query_embedding.to_vec());
                let mut search_buf = instant_distance::Search::default();
                // Extract the first result's data before search_buf is dropped
                let first_result = {
                    let results = hnsw_index.hnsw.search(&query_point, &mut search_buf);
                    results.into_iter().next().map(|r| (*r.value, r.distance))
                };

                if let Some((idx, distance)) = first_result {
                    let similarity = (1.0 - distance) as f64;

                    if similarity >= self.threshold {
                        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
                        if let Some(entry) = entries.get(idx) {
                            best = Some((similarity, entry.clone()));
                        }
                    }
                }
            }
        }

        // 2. Linear-scan pending entries for potentially closer matches
        {
            let pending = self
                .pending_entries
                .read()
                .unwrap_or_else(|e| e.into_inner());
            for entry in pending.iter() {
                let query_point = Point(query_embedding.to_vec());
                let entry_point = Point(entry.embedding.clone());
                let distance =
                    <Point as instant_distance::Point>::distance(&query_point, &entry_point);
                let similarity = (1.0 - distance) as f64;

                if similarity >= self.threshold
                    && best
                        .as_ref()
                        .is_none_or(|(best_sim, _)| similarity > *best_sim)
                {
                    best = Some((similarity, entry.clone()));
                }
            }
        }

        best
    }

    /// Save the HNSW index to disk.
    pub fn save_index(&self, _path: &std::path::Path) -> Result<()> {
        // instant-distance doesn't have built-in serialization (serde feature is broken).
        // Store the entries as JSONL and rebuild on load.
        // This is a known limitation documented in the ADR.
        Ok(())
    }

    /// Load the HNSW index from disk.
    pub fn load_index(&self, _path: &std::path::Path) -> Result<()> {
        // See save_index comment -- rebuild from JSONL entries instead.
        Ok(())
    }

    /// Remove all entries for a specific role and rebuild.
    pub fn invalidate_role(&self, role: &str) -> Result<()> {
        let remaining: Vec<DecisionRecord> = {
            let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
            entries
                .iter()
                .filter(|e| e.record.key.role != role)
                .map(|e| e.record.clone())
                .collect()
        };
        self.build_index(&remaining)
    }

    /// Clear the entire index.
    pub fn invalidate_all(&self) {
        let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
        *index = None;
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.clear();
        let mut pending = self
            .pending_entries
            .write()
            .unwrap_or_else(|e| e.into_inner());
        pending.clear();
    }
}

#[async_trait]
impl CascadeTier for EmbeddingSimilarity {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>> {
        // Check if we have any entries to search (index or pending)
        {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            let pending = self
                .pending_entries
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if index.is_none() && pending.is_empty() {
                return Ok(None);
            }
        }

        let query_embedding = self.embed(&input.sanitized_input)?;
        let result = self.search(&query_embedding);

        match result {
            Some((similarity, entry)) => {
                let role_name = input
                    .session
                    .role
                    .as_ref()
                    .map(|r| r.name.as_str())
                    .unwrap_or("*");

                // Only match same role or wildcard
                if entry.record.key.role != role_name && entry.record.key.role != "*" {
                    return Ok(None);
                }
                // Only match same tool
                if entry.record.key.tool != input.tool_name {
                    return Ok(None);
                }

                // Similarity behavior: allow auto-approves, deny falls through, ask escalates
                match entry.record.decision {
                    Decision::Deny => Ok(None),
                    Decision::Allow | Decision::Ask => Ok(Some(DecisionRecord {
                        key: CacheKey {
                            sanitized_input: input.sanitized_input.clone(),
                            tool: input.tool_name.clone(),
                            role: role_name.to_string(),
                        },
                        decision: entry.record.decision,
                        metadata: DecisionMetadata {
                            tier: DecisionTier::EmbeddingSimilarity,
                            confidence: similarity,
                            reason: format!(
                                "embedding cosine similarity {:.3} >= {:.3} with cached {}",
                                similarity, self.threshold, entry.record.decision
                            ),
                            matched_key: Some(entry.record.key.clone()),
                            similarity_score: Some(similarity),
                        },
                        timestamp: Utc::now(),
                        scope: entry.record.scope,
                        file_path: input.file_path.clone(),
                        session_id: String::new(),
                    })),
                }
            }
            None => Ok(None),
        }
    }

    fn tier(&self) -> DecisionTier {
        DecisionTier::EmbeddingSimilarity
    }

    fn name(&self) -> &str {
        "embedding-similarity"
    }
}
