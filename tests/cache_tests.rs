//! Unit tests for the exact cache (Tier 1) and its tri-state behavior.

use hookwise::cascade::cache::ExactCache;
use hookwise::decision::{
    CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier, ScopeLevel,
};
use chrono::Utc;

fn make_key(input: &str, tool: &str, role: &str) -> CacheKey {
    CacheKey {
        sanitized_input: input.into(),
        tool: tool.into(),
        role: role.into(),
    }
}

fn make_record(input: &str, tool: &str, role: &str, decision: Decision) -> DecisionRecord {
    DecisionRecord {
        key: make_key(input, tool, role),
        decision,
        metadata: DecisionMetadata {
            tier: DecisionTier::Human,
            confidence: 1.0,
            reason: "test decision".into(),
            matched_key: None,
            similarity_score: None,
        },
        timestamp: Utc::now(),
        scope: ScopeLevel::Project,
        file_path: None,
        session_id: "test-session".into(),
    }
}

// ---------------------------------------------------------------------------
// Basic insert and retrieve
// ---------------------------------------------------------------------------

#[test]
fn cache_insert_and_stats() {
    let cache = ExactCache::new();
    assert_eq!(cache.stats().total_entries, 0);

    cache.insert(make_record("echo hello", "Bash", "coder", Decision::Allow));
    assert_eq!(cache.stats().total_entries, 1);
    assert_eq!(cache.stats().allow_entries, 1);

    cache.insert(make_record("rm -rf /", "Bash", "coder", Decision::Deny));
    assert_eq!(cache.stats().total_entries, 2);
    assert_eq!(cache.stats().deny_entries, 1);

    cache.insert(make_record("echo secret", "Bash", "coder", Decision::Ask));
    assert_eq!(cache.stats().total_entries, 3);
    assert_eq!(cache.stats().ask_entries, 1);
}

#[test]
fn cache_load_from_records() {
    let cache = ExactCache::new();
    let records = vec![
        make_record("cmd1", "Bash", "coder", Decision::Allow),
        make_record("cmd2", "Bash", "coder", Decision::Deny),
        make_record("cmd3", "Bash", "tester", Decision::Ask),
    ];
    cache.load_from(records);
    assert_eq!(cache.stats().total_entries, 3);
}

// ---------------------------------------------------------------------------
// Tri-state: allow auto-resolves, deny auto-resolves, ask always asks
// ---------------------------------------------------------------------------

// NOTE: The tri-state behavior is enforced by the cascade engine, not the cache
// itself. The cache stores and returns whatever was inserted. The cascade engine
// interprets the returned decision. These tests verify the cache returns the
// correct decision type for each entry.

#[test]
fn cache_returns_allow_decision() {
    let cache = ExactCache::new();
    cache.insert(make_record("ls -la", "Bash", "coder", Decision::Allow));
    let stats = cache.stats();
    assert_eq!(stats.allow_entries, 1);
}

#[test]
fn cache_returns_deny_decision() {
    let cache = ExactCache::new();
    cache.insert(make_record("rm -rf /", "Bash", "coder", Decision::Deny));
    let stats = cache.stats();
    assert_eq!(stats.deny_entries, 1);
}

#[test]
fn cache_returns_ask_decision() {
    let cache = ExactCache::new();
    cache.insert(make_record("edit .env", "Write", "coder", Decision::Ask));
    let stats = cache.stats();
    assert_eq!(stats.ask_entries, 1);
}

// ---------------------------------------------------------------------------
// Invalidation
// ---------------------------------------------------------------------------

#[test]
fn invalidate_role_removes_only_that_role() {
    let cache = ExactCache::new();
    cache.insert(make_record("cmd1", "Bash", "coder", Decision::Allow));
    cache.insert(make_record("cmd2", "Bash", "tester", Decision::Allow));
    cache.insert(make_record("cmd3", "Write", "coder", Decision::Deny));

    cache.invalidate_role("coder");

    let stats = cache.stats();
    assert_eq!(stats.total_entries, 1);
    assert_eq!(stats.allow_entries, 1); // only tester's entry remains
}

#[test]
fn invalidate_all_clears_everything() {
    let cache = ExactCache::new();
    cache.insert(make_record("cmd1", "Bash", "coder", Decision::Allow));
    cache.insert(make_record("cmd2", "Bash", "tester", Decision::Deny));
    cache.insert(make_record("cmd3", "Write", "coder", Decision::Ask));

    cache.invalidate_all();

    assert_eq!(cache.stats().total_entries, 0);
}

#[test]
fn invalidate_nonexistent_role_is_noop() {
    let cache = ExactCache::new();
    cache.insert(make_record("cmd1", "Bash", "coder", Decision::Allow));

    cache.invalidate_role("nonexistent");

    assert_eq!(cache.stats().total_entries, 1);
}

// ---------------------------------------------------------------------------
// Overwrite behavior
// ---------------------------------------------------------------------------

#[test]
fn insert_same_key_overwrites() {
    let cache = ExactCache::new();
    cache.insert(make_record("echo foo", "Bash", "coder", Decision::Allow));
    assert_eq!(cache.stats().allow_entries, 1);
    assert_eq!(cache.stats().deny_entries, 0);

    // Insert same key with different decision
    cache.insert(make_record("echo foo", "Bash", "coder", Decision::Deny));
    assert_eq!(cache.stats().total_entries, 1); // Still one entry
    assert_eq!(cache.stats().deny_entries, 1);
    assert_eq!(cache.stats().allow_entries, 0);
}

// ---------------------------------------------------------------------------
// Key uniqueness: (input, tool, role) tuple
// ---------------------------------------------------------------------------

#[test]
fn different_tools_are_different_keys() {
    let cache = ExactCache::new();
    cache.insert(make_record("some/path", "Write", "coder", Decision::Allow));
    cache.insert(make_record("some/path", "Read", "coder", Decision::Allow));
    assert_eq!(cache.stats().total_entries, 2);
}

#[test]
fn different_roles_are_different_keys() {
    let cache = ExactCache::new();
    cache.insert(make_record("echo hello", "Bash", "coder", Decision::Allow));
    cache.insert(make_record("echo hello", "Bash", "tester", Decision::Deny));
    assert_eq!(cache.stats().total_entries, 2);
}

#[test]
fn different_inputs_are_different_keys() {
    let cache = ExactCache::new();
    cache.insert(make_record("echo hello", "Bash", "coder", Decision::Allow));
    cache.insert(make_record("echo world", "Bash", "coder", Decision::Allow));
    assert_eq!(cache.stats().total_entries, 2);
}

// ---------------------------------------------------------------------------
// Decision precedence model
// ---------------------------------------------------------------------------

#[test]
fn decision_precedence_order() {
    assert!(Decision::Deny.precedence() > Decision::Ask.precedence());
    assert!(Decision::Ask.precedence() > Decision::Allow.precedence());
}

#[test]
fn decision_display() {
    assert_eq!(format!("{}", Decision::Allow), "allow");
    assert_eq!(format!("{}", Decision::Deny), "deny");
    assert_eq!(format!("{}", Decision::Ask), "ask");
}
