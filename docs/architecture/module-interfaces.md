# Module Interfaces Specification

**Date:** 2026-02-08
**Purpose:** Complete trait signatures, struct definitions, and enum variants for captain-hook. This is the contract all implementers code against.

## Module Overview

```
src/
+-- lib.rs            # Public API re-exports
+-- main.rs           # CLI entry point (clap)
+-- error.rs          # CaptainHookError enum
+-- decision.rs       # Decision, DecisionMetadata, DecisionRecord, CacheKey
+-- config.rs         # PolicyConfig, ConfidenceConfig, RoleDefinition, PathPolicy, SimilarityConfig
+-- sanitize.rs       # Sanitizer trait, SanitizePipeline, AhoCorasickSanitizer, RegexSanitizer, EntropySanitizer
+-- storage.rs        # StorageBackend trait, JsonlStorage
+-- scope.rs          # ScopeLevel, ScopeResolver
+-- session.rs        # SessionContext, SessionManager, RegistrationEntry
+-- cascade.rs        # CascadeRunner, CascadeTier trait
+-- path_policy.rs    # PathPolicyEngine (Tier 0)
+-- cache.rs          # ExactCache (Tier 1)
+-- jaccard.rs        # TokenJaccard (Tier 2a)
+-- embedding.rs      # EmbeddingSimilarity (Tier 2b)
+-- supervisor.rs     # SupervisorBackend trait, UnixSocketSupervisor, ApiSupervisor
+-- human.rs          # HumanBackend, PendingDecision, DecisionQueue
+-- ipc.rs            # IpcRequest, IpcResponse, socket server/client
+-- hook_io.rs        # HookInput, HookOutput, stdin/stdout handling
```

---

## error.rs

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CaptainHookError {
    #[error("session not registered: {session_id}")]
    SessionNotRegistered { session_id: String },

    #[error("session disabled: {session_id}")]
    SessionDisabled { session_id: String },

    #[error("role not found: {role_name}")]
    RoleNotFound { role_name: String },

    #[error("policy file not found: {path}")]
    PolicyNotFound { path: PathBuf },

    #[error("invalid policy: {reason}")]
    InvalidPolicy { reason: String },

    #[error("config parse error in {path}: {reason}")]
    ConfigParse { path: PathBuf, reason: String },

    #[error("storage error: {reason}")]
    Storage { reason: String },

    #[error("index build error: {reason}")]
    IndexBuild { reason: String },

    #[error("embedding error: {reason}")]
    Embedding { reason: String },

    #[error("supervisor error: {reason}")]
    Supervisor { reason: String },

    #[error("supervisor timeout after {timeout_secs}s")]
    SupervisorTimeout { timeout_secs: u64 },

    #[error("human decision timeout after {timeout_secs}s")]
    HumanTimeout { timeout_secs: u64 },

    #[error("ipc error: {reason}")]
    Ipc { reason: String },

    #[error("socket not found at {path}")]
    SocketNotFound { path: PathBuf },

    #[error("registration timeout: waited {waited_secs}s for session {session_id}")]
    RegistrationTimeout { session_id: String, waited_secs: u64 },

    #[error("glob pattern error: {pattern}: {reason}")]
    GlobPattern { pattern: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("api error: status={status}, body={body}")]
    Api { status: u16, body: String },
}

pub type Result<T> = std::result::Result<T, CaptainHookError>;
```

---

## decision.rs

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
}

/// Metadata about how and why a decision was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionMetadata {
    /// Which cascade tier produced this decision.
    pub tier: DecisionTier,

    /// Confidence score from the deciding tier. 1.0 for deterministic tiers
    /// (path policy, exact cache, overrides). 0.0-1.0 for similarity and LLM.
    pub confidence: f64,

    /// Human-readable reason for the decision. Shown to the user in queue mode
    /// and stored in JSONL for audit/review.
    pub reason: String,

    /// For similarity tiers: the cache key of the matched entry.
    pub matched_key: Option<CacheKey>,

    /// For similarity tiers: the similarity score (Jaccard coefficient or cosine similarity).
    pub similarity_score: Option<f64>,
}

/// A unique key identifying a cached decision.
/// The cache is keyed on (sanitized_input, tool, role).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// The sanitized tool input. For Bash: the full command string after sanitization.
    /// For Write/Edit/Read: the file path. For other tools: JSON-serialized input after sanitization.
    pub sanitized_input: String,

    /// The tool name (Bash, Write, Edit, Read, Glob, Grep, Task, etc.)
    pub tool: String,

    /// The role of the session that generated or matched this entry.
    /// "*" means the entry applies to all roles (e.g., sensitive path defaults).
    pub role: String,
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

    /// For Write/Edit tools: the file path (separate from sanitized_input for
    /// structural caching). None for non-file tools.
    pub file_path: Option<String>,

    /// The session ID that triggered this decision (for audit trail).
    pub session_id: String,
}

// Re-export ScopeLevel here for convenience (defined in scope.rs)
pub use crate::scope::ScopeLevel;
```

---

## config.rs

```rust
use globset::GlobSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level project policy configuration.
/// Loaded from `.captain-hook/policy.yml` (project) or `~/.config/captain-hook/org/<org>/policy.yml` (org).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Paths that default to `ask` regardless of role.
    pub sensitive_paths: SensitivePathConfig,

    /// Confidence thresholds per scope level.
    pub confidence: ConfidenceConfig,

    /// Similarity thresholds for Jaccard and embedding tiers.
    pub similarity: SimilarityConfig,

    /// Human decision timeout in seconds. Default: 60.
    pub human_timeout_secs: u64,

    /// Registration wait timeout in seconds. Default: 5.
    pub registration_timeout_secs: u64,

    /// Supervisor backend configuration.
    pub supervisor: SupervisorConfig,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            sensitive_paths: SensitivePathConfig::default(),
            confidence: ConfidenceConfig::default(),
            similarity: SimilarityConfig::default(),
            human_timeout_secs: 60,
            registration_timeout_secs: 5,
            supervisor: SupervisorConfig::default(),
        }
    }
}

/// Sensitive path configuration — paths that default to `ask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivePathConfig {
    /// Glob patterns for paths that trigger `ask` on write.
    pub ask_write: Vec<String>,
}

impl Default for SensitivePathConfig {
    fn default() -> Self {
        Self {
            ask_write: vec![
                ".claude/**".into(),
                ".captain-hook/**".into(),
                ".env*".into(),
                "**/.env*".into(),
                ".git/hooks/**".into(),
                "**/secrets/**".into(),
                "~/.claude/**".into(),
                "~/.config/**".into(),
            ],
        }
    }
}

/// Confidence thresholds per scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceConfig {
    /// Minimum confidence for org-wide auto-decisions.
    pub org: f64,
    /// Minimum confidence for project-level auto-decisions.
    pub project: f64,
    /// Minimum confidence for user-level auto-decisions.
    pub user: f64,
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            org: 0.9,
            project: 0.7,
            user: 0.6,
        }
    }
}

/// Similarity thresholds for Tier 2a (Jaccard) and Tier 2b (embedding).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityConfig {
    /// Minimum Jaccard coefficient for a Tier 2a match. Default: 0.7.
    pub jaccard_threshold: f64,

    /// Minimum cosine similarity for a Tier 2b match. Default: 0.85.
    pub embedding_threshold: f64,

    /// Minimum number of tokens for Jaccard comparison. Default: 3.
    /// Commands with fewer tokens skip directly to Tier 2b.
    pub jaccard_min_tokens: usize,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            jaccard_threshold: 0.7,
            embedding_threshold: 0.85,
            jaccard_min_tokens: 3,
        }
    }
}

/// Supervisor backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend")]
pub enum SupervisorConfig {
    /// Unix socket supervisor (Claude Code subagent).
    #[serde(rename = "socket")]
    Socket {
        /// Socket path. Default: `/tmp/captain-hook-<team-id>.sock`
        socket_path: Option<PathBuf>,
    },
    /// API supervisor (standalone, Anthropic API).
    #[serde(rename = "api")]
    Api {
        /// Anthropic API base URL. Default: `https://api.anthropic.com`.
        api_base_url: Option<String>,
        /// Model to use. Default: `claude-sonnet-4-5-20250929`.
        model: Option<String>,
        /// Maximum tokens for supervisor response. Default: 1024.
        max_tokens: Option<u32>,
    },
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self::Socket { socket_path: None }
    }
}

/// A role definition from `roles.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Role name (e.g., "coder", "tester", "maintainer").
    pub name: String,

    /// Natural language description of the role. Used by the LLM supervisor
    /// for behavioral decisions at Tier 3.
    pub description: String,

    /// Deterministic path policies for this role.
    pub paths: PathPolicyConfig,
}

/// Raw path policy from YAML (string globs, before compilation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPolicyConfig {
    /// Glob patterns the role is allowed to write to.
    pub allow_write: Vec<String>,
    /// Glob patterns the role is denied from writing to.
    pub deny_write: Vec<String>,
    /// Glob patterns the role is allowed to read from.
    pub allow_read: Vec<String>,
}

/// Compiled path policy — globset instances ready for matching.
/// Created from PathPolicyConfig at initialization.
pub struct CompiledPathPolicy {
    /// Compiled allow_write globs.
    pub allow_write: GlobSet,
    /// Compiled deny_write globs.
    pub deny_write: GlobSet,
    /// Compiled allow_read globs.
    pub allow_read: GlobSet,
    /// Compiled sensitive_paths ask_write globs.
    pub sensitive_ask_write: GlobSet,
}

/// Roles configuration loaded from roles.yml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolesConfig {
    pub roles: HashMap<String, RoleDefinition>,
}

/// Global captain-hook configuration from `~/.config/captain-hook/config.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default supervisor backend.
    pub supervisor: SupervisorConfig,
    /// Anthropic API key (if using ApiSupervisor). Can also be set via ANTHROPIC_API_KEY env var.
    pub api_key: Option<String>,
    /// Default embedding model name for fastembed. Default: "BAAI/bge-small-en-v1.5".
    pub embedding_model: Option<String>,
}
```

---

## sanitize.rs

```rust
use aho_corasick::AhoCorasick;
use regex::RegexSet;

/// A single sanitization layer.
pub trait Sanitizer: Send + Sync {
    /// Sanitize the input string, replacing detected secrets with `<REDACTED>`.
    /// Returns the sanitized string. The original is never stored.
    fn sanitize(&self, input: &str) -> String;

    /// Name of this sanitizer layer (for logging/debugging).
    fn name(&self) -> &str;
}

/// Layer 1: Literal prefix matching via aho-corasick.
/// Detects known secret prefixes (sk-ant-, ghp_, AKIA, etc.)
pub struct AhoCorasickSanitizer {
    /// Compiled aho-corasick automaton for prefix detection.
    automaton: AhoCorasick,
    /// The prefix strings (for determining redaction boundaries).
    prefixes: Vec<String>,
}

impl AhoCorasickSanitizer {
    /// Build from a list of known secret prefixes.
    pub fn new(prefixes: Vec<String>) -> Self;
}

impl Sanitizer for AhoCorasickSanitizer {
    fn sanitize(&self, input: &str) -> String;
    fn name(&self) -> &str { "aho-corasick" }
}

/// Layer 2: Positional/contextual pattern matching via RegexSet.
/// Detects bearer tokens, API key assignments, connection strings, etc.
pub struct RegexSanitizer {
    /// Compiled RegexSet for pattern matching.
    regex_set: RegexSet,
    /// Individual compiled regexes (for capture group extraction when redacting).
    patterns: Vec<regex::Regex>,
}

impl RegexSanitizer {
    /// Build from a list of regex pattern strings.
    pub fn new(patterns: Vec<String>) -> Result<Self, CaptainHookError>;
}

impl Sanitizer for RegexSanitizer {
    fn sanitize(&self, input: &str) -> String;
    fn name(&self) -> &str { "regex" }
}

/// Layer 3: Shannon entropy detection for unknown secret formats.
/// Flags 20+ character tokens after '=' or ':' with entropy > 4.0.
pub struct EntropySanitizer {
    /// Minimum token length to consider. Default: 20.
    pub min_length: usize,
    /// Minimum Shannon entropy to flag. Default: 4.0.
    pub min_entropy: f64,
}

impl EntropySanitizer {
    pub fn new(min_length: usize, min_entropy: f64) -> Self;

    /// Calculate Shannon entropy of a string.
    fn shannon_entropy(s: &str) -> f64;
}

impl Sanitizer for EntropySanitizer {
    fn sanitize(&self, input: &str) -> String;
    fn name(&self) -> &str { "entropy" }
}

/// The complete sanitization pipeline. Runs all layers in sequence.
pub struct SanitizePipeline {
    layers: Vec<Box<dyn Sanitizer>>,
}

impl SanitizePipeline {
    /// Create the default pipeline with all three layers and built-in patterns.
    pub fn default_pipeline() -> Self;

    /// Create a pipeline from custom layers.
    pub fn new(layers: Vec<Box<dyn Sanitizer>>) -> Self;

    /// Run all sanitization layers in sequence.
    pub fn sanitize(&self, input: &str) -> String;
}
```

---

## storage.rs

```rust
use std::path::Path;
use crate::decision::{CacheKey, Decision, DecisionRecord, ScopeLevel};
use crate::error::Result;

/// Backend for loading and saving decision records.
pub trait StorageBackend: Send + Sync {
    /// Load all decisions from storage for a given scope.
    /// Returns decisions from all three files (allow.jsonl, deny.jsonl, ask.jsonl).
    fn load_decisions(&self, scope: ScopeLevel) -> Result<Vec<DecisionRecord>>;

    /// Load decisions filtered by role.
    fn load_decisions_for_role(&self, scope: ScopeLevel, role: &str) -> Result<Vec<DecisionRecord>>;

    /// Save a single decision. Appends to the appropriate JSONL file
    /// (allow.jsonl, deny.jsonl, or ask.jsonl) based on the decision type.
    fn save_decision(&self, record: &DecisionRecord) -> Result<()>;

    /// Delete all decisions for a specific role within a scope.
    /// Used by `captain-hook invalidate --role <name>`.
    fn invalidate_role(&self, scope: ScopeLevel, role: &str) -> Result<()>;

    /// Delete all decisions within a scope.
    /// Used by `captain-hook invalidate --all`.
    fn invalidate_all(&self, scope: ScopeLevel) -> Result<()>;

    /// Rebuild the HNSW index from stored decisions.
    /// Used by `captain-hook build`.
    fn rebuild_index(&self, scope: ScopeLevel) -> Result<()>;

    /// Scan stored decisions for secrets that may have bypassed sanitization.
    /// Used by `captain-hook scan --staged`.
    fn scan_for_secrets(&self, path: &Path) -> Result<Vec<SecretFinding>>;
}

/// A potential secret found during scanning.
#[derive(Debug, Clone)]
pub struct SecretFinding {
    /// File path where the secret was found.
    pub file: std::path::PathBuf,
    /// Line number (1-indexed).
    pub line: usize,
    /// Description of what was detected.
    pub description: String,
    /// The sanitizer layer that detected it.
    pub detector: String,
}

/// JSONL-based storage implementation.
/// Reads/writes `.captain-hook/rules/{allow,deny,ask}.jsonl` for project scope,
/// `~/.config/captain-hook/org/<org>/rules/` for org scope,
/// `~/.config/captain-hook/user/rules.jsonl` for user scope.
pub struct JsonlStorage {
    /// Root directory for project-level storage (typically `<repo>/.captain-hook/`).
    project_root: std::path::PathBuf,
    /// Root directory for global config (typically `~/.config/captain-hook/`).
    global_root: std::path::PathBuf,
    /// Organization name (derived from git remote).
    org_name: Option<String>,
}

impl JsonlStorage {
    pub fn new(
        project_root: std::path::PathBuf,
        global_root: std::path::PathBuf,
        org_name: Option<String>,
    ) -> Self;

    /// Resolve the directory path for a given scope.
    fn scope_dir(&self, scope: ScopeLevel) -> std::path::PathBuf;

    /// Resolve the JSONL file path for a given scope and decision type.
    fn jsonl_path(&self, scope: ScopeLevel, decision: Decision) -> std::path::PathBuf;
}

impl StorageBackend for JsonlStorage {
    fn load_decisions(&self, scope: ScopeLevel) -> Result<Vec<DecisionRecord>>;
    fn load_decisions_for_role(&self, scope: ScopeLevel, role: &str) -> Result<Vec<DecisionRecord>>;
    fn save_decision(&self, record: &DecisionRecord) -> Result<()>;
    fn invalidate_role(&self, scope: ScopeLevel, role: &str) -> Result<()>;
    fn invalidate_all(&self, scope: ScopeLevel) -> Result<()>;
    fn rebuild_index(&self, scope: ScopeLevel) -> Result<()>;
    fn scan_for_secrets(&self, path: &Path) -> Result<Vec<SecretFinding>>;
}
```

---

## scope.rs

```rust
use serde::{Deserialize, Serialize};
use crate::decision::{Decision, DecisionRecord};
use crate::error::Result;
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

/// Resolves the effective decision across all scopes.
///
/// Precedence: DENY > ASK > ALLOW > silent
/// A deny at any level is authoritative.
/// An ask at any level is authoritative (unless a higher scope denies).
/// Allow only applies if no higher-priority scope denies or asks.
pub struct ScopeResolver {
    storage: Box<dyn StorageBackend>,
}

impl ScopeResolver {
    pub fn new(storage: Box<dyn StorageBackend>) -> Self;

    /// Resolve the effective decision across all scopes for a given cache key.
    ///
    /// Checks scopes in order: Role -> User -> Project -> Org.
    /// Applies precedence: DENY > ASK > ALLOW > silent.
    ///
    /// Returns None if no scope has a matching decision (novel command).
    pub fn resolve(
        &self,
        key: &crate::decision::CacheKey,
        session: &crate::session::SessionContext,
    ) -> Result<Option<ScopedDecision>>;
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
```

---

## session.rs

```rust
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::LazyLock;

use crate::config::{CompiledPathPolicy, RoleDefinition};
use crate::error::Result;

/// In-memory session context, populated on first tool call from a session.
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// OS user (from whoami or git config user.email).
    pub user: String,

    /// Organization name (from git remote origin URL).
    pub org: String,

    /// Project/repository name (from git remote origin URL).
    pub project: String,

    /// Team ID (from Claude Code team config), if in a team session.
    pub team: Option<String>,

    /// The session's registered role, if any.
    pub role: Option<RoleDefinition>,

    /// Compiled path policy for this session's role.
    /// None if no role is registered.
    pub path_policy: Option<CompiledPathPolicy>,

    /// SHA-256 hash of the agent's system prompt (for integrity checking).
    pub agent_prompt_hash: Option<String>,

    /// Path to the agent's system prompt file (read on-demand by LLM supervisor).
    pub agent_prompt_path: Option<PathBuf>,

    /// The task description the agent was delegated.
    pub task_description: Option<String>,

    /// When this session was registered.
    pub registered_at: Option<DateTime<Utc>>,

    /// Whether captain-hook is disabled for this session.
    pub disabled: bool,
}

/// Global concurrent session cache.
/// Keyed by session_id, populated on first tool call.
pub static SESSIONS: LazyLock<DashMap<String, SessionContext>> =
    LazyLock::new(DashMap::new);

/// Manages session registration and lookup.
pub struct SessionManager {
    /// Path to the registration file.
    /// `/tmp/captain-hook-<team-id>-sessions.json` for teams,
    /// `/tmp/captain-hook-solo-sessions.json` for solo.
    registration_file: PathBuf,

    /// Path to the exclusion (disabled) file.
    exclusion_file: PathBuf,
}

impl SessionManager {
    pub fn new(team_id: Option<&str>) -> Self;

    /// Resolve a session's role. Checks in order:
    /// 1. In-memory cache (SESSIONS DashMap)
    /// 2. Registration file on disk
    /// 3. CAPTAIN_HOOK_ROLE env var
    /// Returns None if session is not registered.
    pub fn resolve_role(&self, session_id: &str) -> Result<Option<RoleDefinition>>;

    /// Get the full session context, populating if needed.
    /// First call for a session: ~10ms (git remote + registration file + team config).
    /// Subsequent calls: nanoseconds (DashMap lookup).
    pub fn get_or_populate(&self, session_id: &str, cwd: &str) -> Result<SessionContext>;

    /// Register a session with a role.
    pub fn register(
        &self,
        session_id: &str,
        role_name: &str,
        task: Option<&str>,
        prompt_file: Option<&str>,
    ) -> Result<()>;

    /// Disable captain-hook for a session.
    pub fn disable(&self, session_id: &str) -> Result<()>;

    /// Re-enable captain-hook for a session.
    pub fn enable(&self, session_id: &str) -> Result<()>;

    /// Switch a session's role. Clears the session's cache entries.
    pub fn switch_role(&self, session_id: &str, new_role: &str) -> Result<()>;

    /// Check if a session is registered (either with a role or disabled).
    pub fn is_registered(&self, session_id: &str) -> bool;

    /// Check if a session is disabled.
    pub fn is_disabled(&self, session_id: &str) -> bool;

    /// Wait for a session to be registered, polling every 200ms.
    /// Returns Ok(()) if registered within timeout, Err(RegistrationTimeout) otherwise.
    pub async fn wait_for_registration(
        &self,
        session_id: &str,
        timeout_secs: u64,
    ) -> Result<()>;
}

/// A registration entry from the on-disk sessions file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationEntry {
    pub role: String,
    pub task: Option<String>,
    pub prompt_hash: Option<String>,
    pub prompt_path: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub registered_by: Option<String>,
}
```

---

## cascade.rs

```rust
use async_trait::async_trait;
use crate::decision::{DecisionRecord, DecisionTier};
use crate::error::Result;
use crate::session::SessionContext;

/// Input to each cascade tier.
#[derive(Debug, Clone)]
pub struct CascadeInput {
    /// The session context for the requesting agent.
    pub session: SessionContext,

    /// The tool name (Bash, Write, Edit, Read, etc.)
    pub tool_name: String,

    /// The raw tool input (JSON value).
    pub tool_input: serde_json::Value,

    /// The sanitized tool input string (after SanitizePipeline).
    pub sanitized_input: String,

    /// Extracted file path (for Write/Edit/Read tools). None for non-file tools.
    pub file_path: Option<String>,
}

/// A single tier in the decision cascade.
#[async_trait]
pub trait CascadeTier: Send + Sync {
    /// Evaluate this tier. Returns Some(record) if the tier can make a decision,
    /// None if it should fall through to the next tier.
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>>;

    /// The tier identifier (for logging and metadata).
    fn tier(&self) -> DecisionTier;

    /// Human-readable name for this tier (for logging).
    fn name(&self) -> &str;
}

/// The complete cascade runner. Evaluates tiers in order until one resolves.
pub struct CascadeRunner {
    /// The sanitization pipeline (runs before all tiers).
    pub sanitizer: crate::sanitize::SanitizePipeline,

    /// Tier 0: Path policy (deterministic glob matching).
    pub path_policy: Box<dyn CascadeTier>,

    /// Tier 1: Exact cache match.
    pub exact_cache: Box<dyn CascadeTier>,

    /// Tier 2a: Token-level Jaccard similarity.
    pub token_jaccard: Box<dyn CascadeTier>,

    /// Tier 2b: Embedding HNSW similarity.
    pub embedding_similarity: Box<dyn CascadeTier>,

    /// Tier 3: LLM supervisor.
    pub supervisor: Box<dyn CascadeTier>,

    /// Tier 4: Human-in-the-loop.
    pub human: Box<dyn CascadeTier>,

    /// Storage backend for persisting decisions.
    pub storage: Box<dyn crate::storage::StorageBackend>,

    /// Policy configuration.
    pub policy: crate::config::PolicyConfig,
}

impl CascadeRunner {
    /// Run the full cascade for a tool call.
    ///
    /// 1. Sanitize the input
    /// 2. Run each tier in order: path_policy -> exact_cache -> token_jaccard
    ///    -> embedding_similarity -> supervisor -> human
    /// 3. If a tier returns Some(record), persist the decision and return it
    /// 4. Special case: if exact_cache returns an `ask` decision, skip tiers 2a-3
    ///    and go directly to human
    ///
    /// Returns the decision record from whichever tier resolved.
    pub async fn evaluate(
        &self,
        session: &SessionContext,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Result<DecisionRecord>;

    /// Extract file path from tool input for file-related tools.
    fn extract_file_path(tool_name: &str, tool_input: &serde_json::Value) -> Option<String>;

    /// Persist a decision to storage and update in-memory caches.
    async fn persist_decision(&self, record: &DecisionRecord) -> Result<()>;
}
```

---

## path_policy.rs

```rust
use crate::cascade::{CascadeInput, CascadeTier};
use crate::config::CompiledPathPolicy;
use crate::decision::{Decision, DecisionRecord, DecisionTier};
use crate::error::Result;

/// Tier 0: Deterministic path policy check.
///
/// For file-related tools (Write, Edit, Read), extracts the file path and
/// checks it against the session's role path policy (compiled globset).
///
/// For Bash commands, attempts to extract file paths via regex patterns
/// for common write commands (rm, mv, cp, mkdir, touch, redirects, sed -i, etc.).
///
/// Evaluation order:
/// 1. Check sensitive_paths ask_write globs -> if match, return Ask
/// 2. Check role deny_write globs -> if match, return Deny
/// 3. Check role allow_write globs -> if match, return Allow
/// 4. If no glob matches, return None (fall through to next tier)
pub struct PathPolicyEngine {
    /// Regex patterns for extracting file paths from Bash commands.
    bash_path_extractors: Vec<regex::Regex>,
}

impl PathPolicyEngine {
    pub fn new() -> Result<Self>;
}

#[async_trait::async_trait]
impl CascadeTier for PathPolicyEngine {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> DecisionTier { DecisionTier::PathPolicy }
    fn name(&self) -> &str { "path-policy" }
}
```

---

## cache.rs

```rust
use std::collections::HashMap;
use std::sync::RwLock;
use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{CacheKey, Decision, DecisionRecord, DecisionTier};
use crate::error::Result;

/// Tier 1: Exact cache lookup.
///
/// HashMap keyed on (sanitized_input, tool, role). O(1) lookup, ~100ns.
///
/// Behavior:
/// - `allow` hit -> return allow (tier resolved)
/// - `deny` hit -> return deny (tier resolved)
/// - `ask` hit -> return ask (cascade runner will skip to human)
/// - miss -> return None (fall through to next tier)
pub struct ExactCache {
    /// The in-memory cache. RwLock for concurrent read access.
    entries: RwLock<HashMap<CacheKey, DecisionRecord>>,
}

impl ExactCache {
    /// Create an empty cache.
    pub fn new() -> Self;

    /// Load cache from stored decisions.
    pub fn load_from(&self, records: Vec<DecisionRecord>);

    /// Insert or update a cache entry.
    pub fn insert(&self, record: DecisionRecord);

    /// Remove all entries for a specific role.
    pub fn invalidate_role(&self, role: &str);

    /// Remove all entries.
    pub fn invalidate_all(&self);

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats;
}

#[async_trait::async_trait]
impl CascadeTier for ExactCache {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> DecisionTier { DecisionTier::ExactCache }
    fn name(&self) -> &str { "exact-cache" }
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
```

---

## jaccard.rs

```rust
use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{CacheKey, DecisionRecord, DecisionTier};
use crate::error::Result;
use std::sync::RwLock;

/// A token set entry for Jaccard comparison.
#[derive(Debug, Clone)]
pub struct TokenEntry {
    /// Sorted, deduplicated, lowercased tokens.
    pub tokens: Vec<String>,

    /// The cache key referencing the cached decision.
    pub cache_key: CacheKey,

    /// The full decision record.
    pub record: DecisionRecord,
}

/// Tier 2a: Token-level Jaccard similarity.
///
/// Tokenizes the sanitized input (split on whitespace + punctuation, lowercase,
/// deduplicate, sort) and computes Jaccard coefficient against all cached entries.
///
/// Commands with fewer than `min_tokens` (default: 3) skip this tier.
///
/// Behavior:
/// - Jaccard >= threshold and matched decision is allow -> auto-approve
/// - Jaccard >= threshold and matched decision is deny -> fall through (return None)
/// - Jaccard >= threshold and matched decision is ask -> return ask (escalate)
/// - Jaccard < threshold for all entries -> return None (fall through)
pub struct TokenJaccard {
    /// All token entries from cached decisions.
    entries: RwLock<Vec<TokenEntry>>,

    /// Jaccard threshold. Default: 0.7.
    threshold: f64,

    /// Minimum token count. Default: 3.
    min_tokens: usize,
}

impl TokenJaccard {
    pub fn new(threshold: f64, min_tokens: usize) -> Self;

    /// Load entries from cached decisions.
    pub fn load_from(&self, records: &[DecisionRecord]);

    /// Add a single entry.
    pub fn insert(&self, record: &DecisionRecord);

    /// Tokenize an input string: split on whitespace + punctuation, lowercase,
    /// deduplicate, sort.
    pub fn tokenize(input: &str) -> Vec<String>;

    /// Compute Jaccard coefficient between two sorted token slices.
    /// Uses merge-join: O(|A| + |B|).
    pub fn jaccard_coefficient(a: &[String], b: &[String]) -> f64;

    /// Count intersection of two sorted slices using merge-join.
    fn sorted_intersection_count(a: &[String], b: &[String]) -> usize;

    /// Remove all entries for a specific role.
    pub fn invalidate_role(&self, role: &str);

    /// Remove all entries.
    pub fn invalidate_all(&self);
}

#[async_trait::async_trait]
impl CascadeTier for TokenJaccard {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> DecisionTier { DecisionTier::TokenJaccard }
    fn name(&self) -> &str { "token-jaccard" }
}
```

---

## embedding.rs

```rust
use crate::cascade::{CascadeInput, CascadeTier};
use crate::decision::{DecisionRecord, DecisionTier};
use crate::error::Result;
use std::sync::RwLock;

/// An entry in the HNSW index.
#[derive(Debug, Clone)]
pub struct EmbeddingEntry {
    /// The embedding vector.
    pub embedding: Vec<f32>,

    /// The full decision record this embedding represents.
    pub record: DecisionRecord,
}

/// Tier 2b: Embedding-based HNSW similarity search.
///
/// Uses fastembed for local embedding generation and instant-distance for
/// HNSW approximate nearest neighbor search.
///
/// Behavior:
/// - Cosine similarity >= threshold and matched decision is allow -> auto-approve
/// - Cosine similarity >= threshold and matched decision is deny -> fall through (return None)
/// - Cosine similarity >= threshold and matched decision is ask -> return ask (escalate)
/// - Below threshold for all entries -> return None (fall through)
pub struct EmbeddingSimilarity {
    /// The HNSW index (instant-distance).
    /// None if no decisions have been indexed yet.
    index: RwLock<Option<HnswIndex>>,

    /// The embedding model (fastembed).
    model: fastembed::TextEmbedding,

    /// Cosine similarity threshold. Default: 0.85.
    threshold: f64,

    /// All indexed entries (for retrieving the decision record by index position).
    entries: RwLock<Vec<EmbeddingEntry>>,
}

/// Wrapper around instant-distance HNSW index with serialization support.
pub struct HnswIndex {
    /// The instant-distance HNSW graph.
    hnsw: instant_distance::HnswMap<Point, usize>,
}

/// A point in the embedding space (wrapper for instant-distance).
#[derive(Clone)]
pub struct Point(pub Vec<f32>);

impl instant_distance::Point for Point {
    fn distance(&self, other: &Self) -> f32;
}

impl EmbeddingSimilarity {
    /// Create a new embedding similarity engine.
    /// Loads the fastembed model (downloads on first use, cached thereafter).
    pub fn new(model_name: &str, threshold: f64) -> Result<Self>;

    /// Build/rebuild the HNSW index from a set of decision records.
    pub fn build_index(&self, records: &[DecisionRecord]) -> Result<()>;

    /// Add a single entry to the index. May trigger a lazy rebuild.
    pub fn insert(&self, record: &DecisionRecord) -> Result<()>;

    /// Generate an embedding for a text input.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Search the index for the nearest neighbor.
    /// Returns the best match above the threshold, or None.
    pub fn search(&self, query_embedding: &[f32]) -> Option<(f64, &EmbeddingEntry)>;

    /// Save the HNSW index to disk.
    pub fn save_index(&self, path: &std::path::Path) -> Result<()>;

    /// Load the HNSW index from disk.
    pub fn load_index(&self, path: &std::path::Path) -> Result<()>;

    /// Remove all entries for a specific role and rebuild.
    pub fn invalidate_role(&self, role: &str) -> Result<()>;

    /// Clear the entire index.
    pub fn invalidate_all(&self);
}

#[async_trait::async_trait]
impl CascadeTier for EmbeddingSimilarity {
    async fn evaluate(&self, input: &CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> DecisionTier { DecisionTier::EmbeddingSimilarity }
    fn name(&self) -> &str { "embedding-similarity" }
}
```

---

## supervisor.rs

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::config::PolicyConfig;
use crate::decision::DecisionRecord;
use crate::error::Result;
use crate::session::SessionContext;

/// Request sent to the supervisor for evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorRequest {
    /// The session requesting the permission.
    pub session_id: String,

    /// The role of the requesting session.
    pub role: String,

    /// Natural language description of the role.
    pub role_description: String,

    /// The tool being invoked.
    pub tool_name: String,

    /// The sanitized tool input.
    pub sanitized_input: String,

    /// File path (if applicable).
    pub file_path: Option<String>,

    /// The task the agent was delegated.
    pub task_description: Option<String>,

    /// Path to the agent's system prompt (for on-demand reading by supervisor).
    pub agent_prompt_path: Option<String>,

    /// Current working directory.
    pub cwd: String,
}

/// Response from the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorResponse {
    /// The decision.
    pub decision: crate::decision::Decision,

    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,

    /// Human-readable reason for the decision.
    pub reason: String,
}

/// Pluggable supervisor backend trait.
#[async_trait]
pub trait SupervisorBackend: Send + Sync {
    /// Evaluate a permission request.
    /// Returns a decision with confidence and reason.
    async fn evaluate(
        &self,
        request: &SupervisorRequest,
        policy: &PolicyConfig,
    ) -> Result<DecisionRecord>;
}

/// Unix socket supervisor — communicates with a Claude Code subagent.
pub struct UnixSocketSupervisor {
    /// Path to the Unix domain socket.
    socket_path: std::path::PathBuf,

    /// Connection timeout in seconds.
    timeout_secs: u64,
}

impl UnixSocketSupervisor {
    pub fn new(socket_path: std::path::PathBuf, timeout_secs: u64) -> Self;
}

#[async_trait]
impl SupervisorBackend for UnixSocketSupervisor {
    async fn evaluate(
        &self,
        request: &SupervisorRequest,
        policy: &PolicyConfig,
    ) -> Result<DecisionRecord>;
}

/// API supervisor — calls the Anthropic API directly.
pub struct ApiSupervisor {
    /// HTTP client.
    client: reqwest::Client,

    /// Anthropic API base URL.
    api_base_url: String,

    /// API key.
    api_key: String,

    /// Model name.
    model: String,

    /// Maximum tokens for the response.
    max_tokens: u32,
}

impl ApiSupervisor {
    pub fn new(
        api_base_url: String,
        api_key: String,
        model: String,
        max_tokens: u32,
    ) -> Self;

    /// Build the system prompt for the supervisor, including the project's
    /// policy, role definitions, and relevant cached decisions.
    fn build_system_prompt(&self, policy: &PolicyConfig) -> String;

    /// Build the user message from the supervisor request.
    fn build_user_message(&self, request: &SupervisorRequest) -> String;

    /// Parse the API response into a SupervisorResponse.
    fn parse_response(&self, response_text: &str) -> Result<SupervisorResponse>;
}

#[async_trait]
impl SupervisorBackend for ApiSupervisor {
    async fn evaluate(
        &self,
        request: &SupervisorRequest,
        policy: &PolicyConfig,
    ) -> Result<DecisionRecord>;
}

/// Wraps a SupervisorBackend as a CascadeTier.
pub struct SupervisorTier {
    backend: Box<dyn SupervisorBackend>,
    policy: PolicyConfig,
}

impl SupervisorTier {
    pub fn new(backend: Box<dyn SupervisorBackend>, policy: PolicyConfig) -> Self;
}

#[async_trait]
impl crate::cascade::CascadeTier for SupervisorTier {
    async fn evaluate(&self, input: &crate::cascade::CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> crate::decision::DecisionTier { crate::decision::DecisionTier::Supervisor }
    fn name(&self) -> &str { "supervisor" }
}
```

---

## human.rs

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use crate::decision::{Decision, DecisionRecord};
use crate::error::Result;

/// A pending decision waiting for human response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDecision {
    /// Unique ID for this pending decision.
    pub id: String,

    /// The session that triggered this decision.
    pub session_id: String,

    /// The role of the requesting session.
    pub role: String,

    /// The tool being invoked.
    pub tool_name: String,

    /// The sanitized tool input.
    pub sanitized_input: String,

    /// File path (if applicable).
    pub file_path: Option<String>,

    /// The supervisor's recommendation (if it reached tier 3).
    pub recommendation: Option<SupervisorRecommendation>,

    /// Whether this is an `ask` re-prompt (cached decision was `ask`).
    pub is_ask_reprompt: bool,

    /// Reason for the `ask` state (if is_ask_reprompt).
    pub ask_reason: Option<String>,

    /// When this decision was queued.
    pub queued_at: DateTime<Utc>,
}

/// The supervisor's recommendation accompanying a human prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorRecommendation {
    pub decision: Decision,
    pub confidence: f64,
    pub reason: String,
}

/// A human's response to a pending decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanResponse {
    /// The decision for the current invocation.
    pub decision: Decision,

    /// Whether to cache this as `ask` (always prompt in the future).
    pub always_ask: bool,

    /// Whether to codify this as a persistent rule.
    pub add_rule: bool,

    /// Which scope to store the rule in.
    pub rule_scope: Option<crate::scope::ScopeLevel>,
}

/// The decision queue for human-in-the-loop interactions.
pub struct DecisionQueue {
    /// Pending decisions keyed by ID.
    pending: RwLock<HashMap<String, PendingDecision>>,

    /// Completed decisions (for the blocked hook to pick up).
    completed: RwLock<HashMap<String, HumanResponse>>,
}

impl DecisionQueue {
    pub fn new() -> Self;

    /// Add a pending decision to the queue. Returns the decision ID.
    pub fn enqueue(&self, decision: PendingDecision) -> String;

    /// List all pending decisions.
    pub fn list_pending(&self) -> Vec<PendingDecision>;

    /// Get a specific pending decision.
    pub fn get_pending(&self, id: &str) -> Option<PendingDecision>;

    /// Submit a human response for a pending decision.
    pub fn respond(&self, id: &str, response: HumanResponse) -> Result<()>;

    /// Wait for a response to a specific pending decision.
    /// Polls every 200ms until a response arrives or timeout.
    pub async fn wait_for_response(
        &self,
        id: &str,
        timeout_secs: u64,
    ) -> Result<HumanResponse>;

    /// Remove a completed decision from the completed map.
    pub fn take_response(&self, id: &str) -> Option<HumanResponse>;
}

/// Tier 4: Human-in-the-loop. Wraps the DecisionQueue as a CascadeTier.
pub struct HumanTier {
    queue: std::sync::Arc<DecisionQueue>,
    timeout_secs: u64,
}

impl HumanTier {
    pub fn new(queue: std::sync::Arc<DecisionQueue>, timeout_secs: u64) -> Self;
}

#[async_trait::async_trait]
impl crate::cascade::CascadeTier for HumanTier {
    async fn evaluate(&self, input: &crate::cascade::CascadeInput) -> Result<Option<DecisionRecord>>;
    fn tier(&self) -> crate::decision::DecisionTier { crate::decision::DecisionTier::Human }
    fn name(&self) -> &str { "human" }
}
```

---

## ipc.rs

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::decision::{Decision, DecisionMetadata};
use crate::error::Result;

/// IPC request sent from worker hook to supervisor via Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    /// The session ID of the requesting worker.
    pub session_id: String,

    /// The tool being invoked.
    pub tool_name: String,

    /// The sanitized tool input.
    pub tool_input: String,

    /// The role of the requesting session.
    pub role: String,

    /// File path (if applicable).
    pub file_path: Option<String>,

    /// The task description for this session.
    pub task_description: Option<String>,

    /// Path to the agent's system prompt file.
    pub prompt_path: Option<String>,

    /// Current working directory.
    pub cwd: String,
}

/// IPC response from supervisor to worker hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    /// The decision.
    pub decision: Decision,

    /// Metadata about the decision.
    pub metadata: DecisionMetadata,
}

/// Unix socket server for the supervisor agent.
/// Listens for IPC requests from worker hooks.
pub struct IpcServer {
    /// Path to the Unix domain socket.
    socket_path: PathBuf,
}

impl IpcServer {
    pub fn new(socket_path: PathBuf) -> Self;

    /// Start listening for connections. Each connection is handled in a spawned task.
    /// The handler receives an IpcRequest and must return an IpcResponse.
    pub async fn serve<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(IpcRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<IpcResponse>> + Send>>
            + Send
            + Sync
            + 'static;

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> Result<()>;
}

/// Unix socket client for worker hooks to connect to the supervisor.
pub struct IpcClient {
    /// Path to the Unix domain socket.
    socket_path: PathBuf,

    /// Connection timeout.
    timeout_secs: u64,
}

impl IpcClient {
    pub fn new(socket_path: PathBuf, timeout_secs: u64) -> Self;

    /// Send a request and wait for a response.
    pub async fn request(&self, req: &IpcRequest) -> Result<IpcResponse>;
}
```

---

## hook_io.rs

```rust
use serde::{Deserialize, Serialize};
use crate::error::Result;

/// The JSON payload Claude Code sends to hooks on stdin.
///
/// For `PreToolUse` hooks, this contains the session_id, tool_name,
/// tool_input, and other context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// The Claude Code session ID.
    pub session_id: String,

    /// The tool being invoked (Bash, Write, Edit, Read, Glob, Grep, Task, etc.)
    pub tool_name: String,

    /// The tool input as a JSON value.
    /// For Bash: {"command": "..."}.
    /// For Write: {"file_path": "...", "content": "..."}.
    /// For Edit: {"file_path": "...", "old_string": "...", "new_string": "..."}.
    /// For Read: {"file_path": "..."}.
    pub tool_input: serde_json::Value,

    /// The current working directory.
    pub cwd: String,

    /// The permission mode set by the user (default, plan, etc.)
    #[serde(default)]
    pub permission_mode: Option<String>,
}

/// The JSON payload captain-hook outputs to stdout.
///
/// Claude Code reads the `hookSpecificOutput.permissionDecision` field
/// to determine whether to allow, deny, or ask about the tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutput {
    /// Hook-specific output consumed by Claude Code.
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

/// The permission decision output within HookOutput.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    /// The permission decision: "allow", "deny", or "ask".
    #[serde(rename = "permissionDecision")]
    pub permission_decision: String,
}

impl HookOutput {
    /// Create a new HookOutput with the given decision.
    pub fn new(decision: crate::decision::Decision) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                permission_decision: match decision {
                    crate::decision::Decision::Allow => "allow".to_string(),
                    crate::decision::Decision::Deny => "deny".to_string(),
                    crate::decision::Decision::Ask => "ask".to_string(),
                },
            },
        }
    }
}

/// Read the hook input from stdin.
pub fn read_hook_input() -> Result<HookInput> {
    let stdin = std::io::stdin();
    let input: HookInput = serde_json::from_reader(stdin.lock())?;
    Ok(input)
}

/// Write the hook output to stdout.
pub fn write_hook_output(output: &HookOutput) -> Result<()> {
    let stdout = std::io::stdout();
    serde_json::to_writer(stdout.lock(), output)?;
    Ok(())
}
```

---

## main.rs (CLI structure)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "captain-hook")]
#[command(about = "Intelligent permission gating for Claude Code")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Evaluate a tool call (hook mode). Reads JSON from stdin, writes JSON to stdout.
    Check,

    /// Check if session is registered (user_prompt_submit hook).
    SessionCheck,

    /// Register a session with a role.
    Register {
        /// Claude Code session ID.
        #[arg(long)]
        session_id: String,

        /// Role name from roles.yml.
        #[arg(long)]
        role: String,

        /// Task description.
        #[arg(long)]
        task: Option<String>,

        /// Path to the agent's system prompt.
        #[arg(long)]
        prompt_file: Option<String>,
    },

    /// Disable captain-hook for a session.
    Disable {
        #[arg(long)]
        session_id: String,
    },

    /// Re-enable captain-hook for a disabled session.
    Enable {
        #[arg(long)]
        session_id: String,
    },

    /// List pending permission decisions.
    Queue,

    /// Approve a pending decision.
    Approve {
        /// Pending decision ID.
        id: String,

        /// Cache as `ask` instead of `allow`.
        #[arg(long)]
        always_ask: bool,

        /// Codify as a persistent rule.
        #[arg(long)]
        add_rule: bool,

        /// Scope for the rule.
        #[arg(long, default_value = "project")]
        scope: String,
    },

    /// Deny a pending decision.
    Deny {
        /// Pending decision ID.
        id: String,

        /// Cache as `ask` instead of `deny`.
        #[arg(long)]
        always_ask: bool,

        /// Codify as a persistent rule.
        #[arg(long)]
        add_rule: bool,

        /// Scope for the rule.
        #[arg(long, default_value = "project")]
        scope: String,
    },

    /// Rebuild vector indexes from rules.
    Build,

    /// Clear cached decisions.
    Invalidate {
        /// Invalidate for a specific role.
        #[arg(long)]
        role: Option<String>,

        /// Invalidate for a specific scope.
        #[arg(long)]
        scope: Option<String>,

        /// Invalidate everything.
        #[arg(long)]
        all: bool,
    },

    /// Set an explicit permission override.
    Override {
        /// Role to override for.
        #[arg(long)]
        role: String,

        /// Command pattern.
        #[arg(long)]
        command: Option<String>,

        /// Tool name.
        #[arg(long)]
        tool: Option<String>,

        /// File path pattern.
        #[arg(long)]
        file: Option<String>,

        /// Allow the operation.
        #[arg(long, group = "decision")]
        allow: bool,

        /// Deny the operation.
        #[arg(long, group = "decision")]
        deny: bool,

        /// Always ask about the operation.
        #[arg(long, group = "decision")]
        ask: bool,

        /// Scope for the override.
        #[arg(long, default_value = "project")]
        scope: String,
    },

    /// Stream decisions in real time.
    Monitor,

    /// Show cache hit rates and decision distribution.
    Stats,

    /// Pre-commit secret scan on staged files.
    Scan {
        /// Only scan staged files.
        #[arg(long)]
        staged: bool,

        /// Path to scan.
        path: Option<String>,
    },

    /// Initialize .captain-hook/ in the current repo.
    Init,

    /// View/edit global configuration.
    Config,

    /// Pull latest org-level rules.
    Sync,
}
```

---

## lib.rs (public API)

```rust
// Re-export all public types for use by other crates or integration tests.

pub mod error;
pub mod decision;
pub mod config;
pub mod sanitize;
pub mod storage;
pub mod scope;
pub mod session;
pub mod cascade;
pub mod path_policy;
pub mod cache;
pub mod jaccard;
pub mod embedding;
pub mod supervisor;
pub mod human;
pub mod ipc;
pub mod hook_io;

pub use error::{CaptainHookError, Result};
pub use decision::{Decision, DecisionMetadata, DecisionRecord, DecisionTier, CacheKey};
pub use config::{PolicyConfig, RoleDefinition, CompiledPathPolicy};
pub use session::{SessionContext, SessionManager};
pub use cascade::CascadeRunner;
pub use hook_io::{HookInput, HookOutput};
```
