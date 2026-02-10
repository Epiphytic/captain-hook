//! Unit tests for path policy: globset matching, deny-wins, sensitive paths.

use hookwise::config::roles::{CompiledPathPolicy, PathPolicyConfig};

fn compile_policy(
    allow_write: Vec<&str>,
    deny_write: Vec<&str>,
    allow_read: Vec<&str>,
    sensitive: Vec<&str>,
) -> CompiledPathPolicy {
    let config = PathPolicyConfig {
        allow_write: allow_write.into_iter().map(String::from).collect(),
        deny_write: deny_write.into_iter().map(String::from).collect(),
        allow_read: allow_read.into_iter().map(String::from).collect(),
    };
    let sensitive_patterns: Vec<String> = sensitive.into_iter().map(String::from).collect();
    CompiledPathPolicy::compile(&config, &sensitive_patterns).unwrap()
}

// ---------------------------------------------------------------------------
// Basic globset matching
// ---------------------------------------------------------------------------

#[test]
fn allow_write_matches_src() {
    let policy = compile_policy(vec!["src/**"], vec![], vec!["**"], vec![]);
    assert!(policy.allow_write.is_match("src/main.rs"));
    assert!(policy.allow_write.is_match("src/lib/nested.rs"));
    assert!(!policy.allow_write.is_match("tests/test.rs"));
}

#[test]
fn deny_write_matches_tests() {
    let policy = compile_policy(vec!["src/**"], vec!["tests/**"], vec!["**"], vec![]);
    assert!(policy.deny_write.is_match("tests/unit.rs"));
    assert!(policy.deny_write.is_match("tests/integration/test.rs"));
    assert!(!policy.deny_write.is_match("src/main.rs"));
}

#[test]
fn allow_read_wildcard_matches_everything() {
    let policy = compile_policy(vec![], vec![], vec!["**"], vec![]);
    assert!(policy.allow_read.is_match("src/main.rs"));
    assert!(policy.allow_read.is_match("docs/README.md"));
    assert!(policy.allow_read.is_match("any/path/at/all"));
}

#[test]
fn sensitive_matches_env_files() {
    let policy = compile_policy(vec!["**"], vec![], vec!["**"], vec![".env*", "**/.env*"]);
    assert!(policy.sensitive_ask_write.is_match(".env"));
    assert!(policy.sensitive_ask_write.is_match(".env.local"));
    assert!(policy
        .sensitive_ask_write
        .is_match("config/.env.production"));
}

#[test]
fn sensitive_matches_claude_dir() {
    let policy = compile_policy(vec!["**"], vec![], vec!["**"], vec![".claude/**"]);
    assert!(policy.sensitive_ask_write.is_match(".claude/settings.json"));
    assert!(policy
        .sensitive_ask_write
        .is_match(".claude/permissions.yml"));
    assert!(!policy.sensitive_ask_write.is_match("src/claude.rs"));
}

// ---------------------------------------------------------------------------
// Coder role patterns (realistic scenario)
// ---------------------------------------------------------------------------

#[test]
fn coder_role_allows_src_denies_tests() {
    let policy = compile_policy(
        vec!["src/**", "lib/**", "Cargo.toml", "package.json"],
        vec!["tests/**", "docs/**", ".github/**"],
        vec!["**"],
        vec![".claude/**", ".env*"],
    );

    // allow_write
    assert!(policy.allow_write.is_match("src/main.rs"));
    assert!(policy.allow_write.is_match("lib/utils.rs"));
    assert!(policy.allow_write.is_match("Cargo.toml"));

    // deny_write
    assert!(policy.deny_write.is_match("tests/unit.rs"));
    assert!(policy.deny_write.is_match("docs/README.md"));
    assert!(policy.deny_write.is_match(".github/workflows/ci.yml"));

    // sensitive
    assert!(policy.sensitive_ask_write.is_match(".claude/CLAUDE.md"));
    assert!(policy.sensitive_ask_write.is_match(".env"));
}

// ---------------------------------------------------------------------------
// Tester role patterns
// ---------------------------------------------------------------------------

#[test]
fn tester_role_allows_tests_denies_src() {
    let policy = compile_policy(
        vec!["tests/**", "test-fixtures/**"],
        vec!["src/**", "lib/**", "docs/**", ".github/**"],
        vec!["**"],
        vec![],
    );

    assert!(policy.allow_write.is_match("tests/integration.rs"));
    assert!(policy.allow_write.is_match("test-fixtures/data.json"));
    assert!(policy.deny_write.is_match("src/main.rs"));
    assert!(policy.deny_write.is_match("lib/core.rs"));
}

// ---------------------------------------------------------------------------
// Knowledge role patterns (e.g. researcher)
// ---------------------------------------------------------------------------

#[test]
fn researcher_role_writes_only_docs_research() {
    let policy = compile_policy(
        vec!["docs/research/**"],
        vec!["src/**", "lib/**", "tests/**", ".github/**"],
        vec!["**"],
        vec![],
    );

    assert!(policy.allow_write.is_match("docs/research/findings.md"));
    assert!(!policy.allow_write.is_match("docs/architecture/overview.md"));
    assert!(policy.deny_write.is_match("src/main.rs"));
}

// ---------------------------------------------------------------------------
// Deny-wins precedence (when both allow and deny match a path)
// ---------------------------------------------------------------------------

#[test]
fn deny_and_allow_both_match_same_path() {
    // If allow_write = "**" (everything) and deny_write = "tests/**"
    // then tests/foo.rs matches both, but deny should win in the cascade logic.
    let policy = compile_policy(vec!["**"], vec!["tests/**"], vec!["**"], vec![]);
    let path = "tests/foo.rs";
    // Both match
    assert!(policy.allow_write.is_match(path));
    assert!(policy.deny_write.is_match(path));
    // Deny should be checked first in the cascade engine (deny wins)
}

// ---------------------------------------------------------------------------
// Sensitive + deny overlap
// ---------------------------------------------------------------------------

#[test]
fn sensitive_and_deny_both_match() {
    // If both sensitive and deny match, the cascade engine should return deny
    // (deny > ask > allow)
    let policy = compile_policy(vec!["**"], vec![".env*"], vec!["**"], vec![".env*"]);
    let path = ".env.local";
    assert!(policy.deny_write.is_match(path));
    assert!(policy.sensitive_ask_write.is_match(path));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_patterns_match_nothing() {
    let policy = compile_policy(vec![], vec![], vec![], vec![]);
    assert!(!policy.allow_write.is_match("anything"));
    assert!(!policy.deny_write.is_match("anything"));
    assert!(!policy.allow_read.is_match("anything"));
    assert!(!policy.sensitive_ask_write.is_match("anything"));
}

#[test]
fn deeply_nested_paths() {
    let policy = compile_policy(vec!["src/**"], vec![], vec!["**"], vec![]);
    assert!(policy.allow_write.is_match("src/a/b/c/d/e/f/g.rs"));
}

#[test]
fn exact_file_glob() {
    let policy = compile_policy(vec!["Cargo.toml"], vec![], vec!["**"], vec![]);
    assert!(policy.allow_write.is_match("Cargo.toml"));
    assert!(!policy.allow_write.is_match("other/Cargo.toml"));
}

#[test]
fn invalid_glob_pattern_returns_error() {
    let config = PathPolicyConfig {
        allow_write: vec!["[invalid".into()],
        deny_write: vec![],
        allow_read: vec!["**".into()],
    };
    let result = CompiledPathPolicy::compile(&config, &[]);
    assert!(result.is_err());
}

#[test]
fn default_sensitive_paths() {
    // Test the default sensitive path patterns from PolicyConfig
    use hookwise::config::policy::SensitivePathConfig;
    let defaults = SensitivePathConfig::default();
    let config = PathPolicyConfig {
        allow_write: vec!["**".into()],
        deny_write: vec![],
        allow_read: vec!["**".into()],
    };
    let policy = CompiledPathPolicy::compile(&config, &defaults.ask_write).unwrap();

    assert!(policy.sensitive_ask_write.is_match(".claude/CLAUDE.md"));
    assert!(policy
        .sensitive_ask_write
        .is_match(".hookwise/policy.yml"));
    assert!(policy.sensitive_ask_write.is_match(".env"));
    assert!(policy.sensitive_ask_write.is_match(".env.local"));
    assert!(policy.sensitive_ask_write.is_match(".git/hooks/pre-commit"));
    assert!(policy
        .sensitive_ask_write
        .is_match("config/secrets/api.key"));
}

// ---------------------------------------------------------------------------
// Maintainer role: full access
// ---------------------------------------------------------------------------

#[test]
fn maintainer_role_writes_anywhere() {
    let policy = compile_policy(vec!["**"], vec![], vec!["**"], vec![]);
    assert!(policy.allow_write.is_match("src/main.rs"));
    assert!(policy.allow_write.is_match("tests/test.rs"));
    assert!(policy.allow_write.is_match(".github/workflows/ci.yml"));
    assert!(policy.allow_write.is_match("any/path"));
    // No deny patterns means nothing is denied
    assert!(!policy.deny_write.is_match("src/main.rs"));
}
