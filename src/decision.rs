use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::scope::ScopeLevel;

/// The three possible permission states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
    Ask,
}

/// Which tier of the cascade produced this decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionTier {
    /// Tier 0: deterministic path policy glob match
    PathPolicy,
    /// Tier 1: exact cache match (HashMap)
    ExactCache,
    /// Tier 2a: token-level Jaccard similarity
    TokenJaccard,
    /// Tier 2b: embedding HNSW similarity
    EmbeddingSimilarity,
    /// Tier 3: LLM supervisor evaluation
    Supervisor,
    /// Tier 4: human-in-the-loop
    Human,
    /// Sensitive path default (pre-cascade)
    SensitivePath,
    /// Explicit override (human-set, deterministic)
    Override,
    /// Default fallback when no cascade tier resolved
    Default,
}

/// Metadata about how and why a decision was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionMetadata {
    /// Which cascade tier produced this decision.
    pub tier: DecisionTier,

    /// Confidence score from the deciding tier. 1.0 for deterministic tiers.
    pub confidence: f64,

    /// Human-readable reason for the decision.
    pub reason: String,

    /// For similarity tiers: the cache key of the matched entry.
    pub matched_key: Option<CacheKey>,

    /// For similarity tiers: the similarity score.
    pub similarity_score: Option<f64>,
}

/// A unique key identifying a cached decision.
/// The cache is keyed on (sanitized_input, tool, role).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// The sanitized tool input.
    pub sanitized_input: String,

    /// The tool name (Bash, Write, Edit, Read, Glob, Grep, Task, etc.)
    pub tool: String,

    /// The role of the session that generated or matched this entry.
    pub role: String,
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::Allow => write!(f, "allow"),
            Decision::Deny => write!(f, "deny"),
            Decision::Ask => write!(f, "ask"),
        }
    }
}

impl Decision {
    /// Returns the precedence rank (higher = more authoritative).
    /// DENY > ASK > ALLOW
    pub fn precedence(&self) -> u8 {
        match self {
            Decision::Deny => 3,
            Decision::Ask => 2,
            Decision::Allow => 1,
        }
    }
}

impl std::fmt::Display for ScopeLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeLevel::Org => write!(f, "org"),
            ScopeLevel::Project => write!(f, "project"),
            ScopeLevel::User => write!(f, "user"),
            ScopeLevel::Role => write!(f, "role"),
        }
    }
}

impl std::str::FromStr for ScopeLevel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "org" => Ok(ScopeLevel::Org),
            "project" => Ok(ScopeLevel::Project),
            "user" => Ok(ScopeLevel::User),
            "role" => Ok(ScopeLevel::Role),
            _ => Err(format!("unknown scope: {s}")),
        }
    }
}

/// A complete decision record, stored in JSONL and used in the cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// The cache key for this decision.
    pub key: CacheKey,

    /// The decision: allow, deny, or ask.
    pub decision: Decision,

    /// Metadata about how the decision was made.
    pub metadata: DecisionMetadata,

    /// When this decision was made.
    pub timestamp: DateTime<Utc>,

    /// Which scope this decision belongs to.
    pub scope: ScopeLevel,

    /// For Write/Edit tools: the file path.
    pub file_path: Option<String>,

    /// The session ID that triggered this decision (for audit trail).
    pub session_id: String,
}
