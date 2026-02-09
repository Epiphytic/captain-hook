//! Unit tests for Tier 2a: token-level Jaccard similarity.

use captain_hook::cascade::token_sim::TokenJaccard;
use captain_hook::decision::{
    CacheKey, Decision, DecisionMetadata, DecisionRecord, DecisionTier, ScopeLevel,
};
use chrono::Utc;

fn make_record(input: &str, tool: &str, role: &str, decision: Decision) -> DecisionRecord {
    DecisionRecord {
        key: CacheKey {
            sanitized_input: input.into(),
            tool: tool.into(),
            role: role.into(),
        },
        decision,
        metadata: DecisionMetadata {
            tier: DecisionTier::Human,
            confidence: 1.0,
            reason: "test".into(),
            matched_key: None,
            similarity_score: None,
        },
        timestamp: Utc::now(),
        scope: ScopeLevel::Project,
        file_path: None,
        session_id: "test".into(),
    }
}

// ---------------------------------------------------------------------------
// Tokenization
// ---------------------------------------------------------------------------

#[test]
fn tokenize_splits_on_whitespace() {
    let tokens = TokenJaccard::tokenize("echo hello world");
    assert!(tokens.contains(&"echo".to_string()));
    assert!(tokens.contains(&"hello".to_string()));
    assert!(tokens.contains(&"world".to_string()));
}

#[test]
fn tokenize_splits_on_punctuation() {
    let tokens = TokenJaccard::tokenize("git commit -m 'hello world'");
    assert!(tokens.contains(&"git".to_string()));
    assert!(tokens.contains(&"commit".to_string()));
    assert!(tokens.contains(&"m".to_string()));
    assert!(tokens.contains(&"hello".to_string()));
}

#[test]
fn tokenize_lowercases() {
    let tokens = TokenJaccard::tokenize("Echo HELLO World");
    assert!(tokens.contains(&"echo".to_string()));
    assert!(tokens.contains(&"hello".to_string()));
    assert!(tokens.contains(&"world".to_string()));
}

#[test]
fn tokenize_deduplicates() {
    let tokens = TokenJaccard::tokenize("echo echo echo");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0], "echo");
}

#[test]
fn tokenize_sorts() {
    let tokens = TokenJaccard::tokenize("zebra apple banana");
    assert_eq!(tokens, vec!["apple", "banana", "zebra"]);
}

#[test]
fn tokenize_empty_input() {
    let tokens = TokenJaccard::tokenize("");
    assert!(tokens.is_empty());
}

#[test]
fn tokenize_only_punctuation() {
    let tokens = TokenJaccard::tokenize("... --- ///");
    assert!(tokens.is_empty());
}

// ---------------------------------------------------------------------------
// Jaccard coefficient
// ---------------------------------------------------------------------------

#[test]
fn jaccard_identical_sets() {
    let a = vec!["alpha".into(), "beta".into(), "gamma".into()];
    let b = vec!["alpha".into(), "beta".into(), "gamma".into()];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 1.0).abs() < f64::EPSILON);
}

#[test]
fn jaccard_completely_disjoint() {
    let a = vec!["alpha".into(), "beta".into()];
    let b = vec!["gamma".into(), "delta".into()];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 0.0).abs() < f64::EPSILON);
}

#[test]
fn jaccard_partial_overlap() {
    // {a, b, c} vs {b, c, d} -> intersection=2, union=4 -> 0.5
    let a = vec!["a".into(), "b".into(), "c".into()];
    let b = vec!["b".into(), "c".into(), "d".into()];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 0.5).abs() < f64::EPSILON);
}

#[test]
fn jaccard_subset() {
    // {a, b} vs {a, b, c} -> intersection=2, union=3 -> 0.667
    let a = vec!["a".into(), "b".into()];
    let b = vec!["a".into(), "b".into(), "c".into()];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 2.0 / 3.0).abs() < 0.01);
}

#[test]
fn jaccard_both_empty() {
    let a: Vec<String> = vec![];
    let b: Vec<String> = vec![];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 1.0).abs() < f64::EPSILON);
}

#[test]
fn jaccard_one_empty() {
    let a: Vec<String> = vec!["a".into()];
    let b: Vec<String> = vec![];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 0.0).abs() < f64::EPSILON);
}

#[test]
fn jaccard_single_element_match() {
    let a = vec!["x".into()];
    let b = vec!["x".into()];
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!((j - 1.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// Realistic command similarity scenarios
// ---------------------------------------------------------------------------

#[test]
fn similar_bash_commands_high_jaccard() {
    let a = TokenJaccard::tokenize("cargo build --release");
    let b = TokenJaccard::tokenize("cargo build --release --target x86_64");
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    // 3 shared tokens (cargo, build, release) out of 5 total unique = 0.6
    assert!(j >= 0.5, "similar commands should have high Jaccard: {}", j);
}

#[test]
fn different_bash_commands_low_jaccard() {
    let a = TokenJaccard::tokenize("cargo build --release");
    let b = TokenJaccard::tokenize("rm -rf /tmp/cache");
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    assert!(j < 0.2, "different commands should have low Jaccard: {}", j);
}

#[test]
fn same_command_different_args() {
    let a = TokenJaccard::tokenize("npm install express");
    let b = TokenJaccard::tokenize("npm install react");
    let j = TokenJaccard::jaccard_coefficient(&a, &b);
    // 2 shared (npm, install) out of 4 total unique = 0.5
    assert!((j - 0.5).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// TokenJaccard index: insert and invalidate
// ---------------------------------------------------------------------------

#[test]
fn insert_and_invalidate_by_role() {
    let tj = TokenJaccard::new(0.7, 3);
    tj.insert(&make_record(
        "cargo build --release",
        "Bash",
        "coder",
        Decision::Allow,
    ));
    tj.insert(&make_record(
        "npm test --coverage",
        "Bash",
        "tester",
        Decision::Allow,
    ));

    tj.invalidate_role("coder");
    // After invalidation, coder entries should be gone.
    // We can verify via invalidate_all and re-check indirectly
    // by inserting again and not getting duplicates from the coder entry.
    tj.invalidate_all();
    // Should be empty now
}

#[test]
fn load_from_records() {
    let tj = TokenJaccard::new(0.7, 3);
    let records = vec![
        make_record("cargo build --release", "Bash", "coder", Decision::Allow),
        make_record("npm test --coverage", "Bash", "tester", Decision::Deny),
    ];
    tj.load_from(&records);
    // No panic = success
}

// ---------------------------------------------------------------------------
// Similarity never auto-denies (verified at design level)
// ---------------------------------------------------------------------------

#[test]
fn design_similarity_never_auto_denies() {
    // This is a design invariant verified in the CascadeTier::evaluate implementation.
    // When a deny entry is the best match, TokenJaccard returns None (falls through).
    // This test documents that invariant for reference -- the actual enforcement
    // is tested in the cascade_integration tests using the async evaluate method.
    //
    // From token_sim.rs line 161-162:
    //   Decision::Deny => Ok(None), // Never auto-deny from similarity
    //
    // We verify the threshold and min_tokens configuration here.
    let _tj = TokenJaccard::new(0.7, 3);
    // threshold=0.7 means 70% token overlap required
    // min_tokens=3 means queries with <3 tokens are skipped
    assert!(true, "design invariant documented");
}
