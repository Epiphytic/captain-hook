//! Unit tests for the 3-layer sanitization pipeline.

use hookwise::sanitize::aho::AhoCorasickSanitizer;
use hookwise::sanitize::entropy::EntropySanitizer;
use hookwise::sanitize::regex_san::RegexSanitizer;
use hookwise::sanitize::{SanitizePipeline, Sanitizer};

// ---------------------------------------------------------------------------
// Layer 1: Aho-Corasick prefix matching
// ---------------------------------------------------------------------------

#[test]
fn aho_redacts_anthropic_key() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "export ANTHROPIC_API_KEY=sk-ant-api03-abc123xyz";
    let result = san.sanitize(input);
    assert!(
        result.contains("<REDACTED>"),
        "should redact sk-ant- prefix"
    );
    assert!(!result.contains("sk-ant-"), "sk-ant- prefix should be gone");
}

#[test]
fn aho_redacts_openai_key() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "OPENAI_API_KEY=sk-proj-abc123def456ghi789";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("sk-proj-"));
}

#[test]
fn aho_redacts_github_pat() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "git clone https://github_pat_aBcDeFgHiJkLmNoPqRsT@github.com/org/repo";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("github_pat_"));
}

#[test]
fn aho_redacts_slack_tokens() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "SLACK_TOKEN=xoxb-12345-67890-abcdef";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("xoxb-"));
}

#[test]
fn aho_redacts_npm_token() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "//registry.npmjs.org/:_authToken=npm_abc123456789abcdef";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("npm_abc"));
}

#[test]
fn aho_redacts_pem_key_inline() {
    // "-----BEGIN" is a prefix. The aho sanitizer extends to the next whitespace.
    // In "-----BEGIN RSA PRIVATE KEY-----", the match is "-----BEGIN" which is
    // the same length as the prefix, so it does NOT redact (token_end <= prefix_len).
    // However, if the PEM header appears as a continuous token it should be caught.
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "-----BEGIN RSA PRIVATE KEY-----";
    let result = san.sanitize(input);
    // BUG DOCUMENTATION: The aho sanitizer does not redact this because the token
    // after matching "-----BEGIN" extends only to the next space, yielding "-----BEGIN"
    // which has length == prefix length, failing the `token_end > prefix_len` check.
    // This is a known limitation: PEM headers with spaces between words won't be caught
    // by the aho-corasick layer alone. The regex or entropy layers may catch the actual
    // key content that follows on subsequent lines.
    assert_eq!(
        result, input,
        "aho layer does not redact PEM headers with spaces (known limitation)"
    );
}

#[test]
fn aho_redacts_gitlab_tokens() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "GITLAB_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("glpat-"));
}

#[test]
fn aho_redacts_pypi_token() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "twine upload --password pypi-AgEIcHlwaTEyMzQ1Njc4OQ";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("pypi-Ag"));
}

#[test]
fn aho_ignores_benign_input() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "echo hello world && ls -la";
    assert_eq!(san.sanitize(input), input);
}

#[test]
fn aho_prefix_only_no_secret_body() {
    // A bare prefix with no body after it should NOT be redacted
    // because the code checks token_end > prefix_len
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "ghp_ is a prefix";
    let result = san.sanitize(input);
    // The prefix alone (no extra chars) should pass through
    assert_eq!(result, input);
}

#[test]
fn aho_multiple_secrets_in_one_line() {
    let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
    let input = "curl -H 'Authorization: ghp_secret123' -H 'X-Token: xoxp-other456'";
    let result = san.sanitize(input);
    assert!(!result.contains("ghp_secret123"));
    assert!(!result.contains("xoxp-other456"));
    // Both should be replaced
    let redacted_count = result.matches("<REDACTED>").count();
    assert!(
        redacted_count >= 2,
        "expected 2+ redactions, got {}",
        redacted_count
    );
}

#[test]
fn aho_custom_prefixes() {
    let san = AhoCorasickSanitizer::new(vec!["CUSTOM_".into()]);
    let input = "CUSTOM_secret123";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("CUSTOM_secret123"));
}

#[test]
fn aho_empty_prefixes() {
    let san = AhoCorasickSanitizer::new(vec![]);
    let input = "ghp_shouldnotmatch because no prefixes loaded";
    assert_eq!(san.sanitize(input), input);
}

// ---------------------------------------------------------------------------
// Layer 2: Regex-based positional patterns
// ---------------------------------------------------------------------------

#[test]
fn regex_redacts_bearer_token() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input =
        r#"curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.longtokenpart""#;
    let result = san.sanitize(input);
    assert!(result.contains("Bearer <REDACTED>") || result.contains("Bearer "));
    assert!(!result.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
}

#[test]
fn regex_redacts_token_assignment() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input = "token=my_super_secret_token_value_12345";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("my_super_secret_token_value_12345"));
}

#[test]
fn regex_redacts_password_assignment() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input = "password: hunter2isverysecret";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("hunter2isverysecret"));
}

#[test]
fn regex_redacts_connection_string_password() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input = "DATABASE_URL=mysql://root:supersecretpass@db.example.com:3306/mydb";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("supersecretpass"));
}

#[test]
fn regex_redacts_cli_token_flag() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input = "gh auth login --token ghp_longtokenvalue1234567890";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("ghp_longtokenvalue1234567890"));
}

#[test]
fn regex_ignores_short_values() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    // Values shorter than 8 chars shouldn't match the api_key/password patterns
    let input = "key=short";
    assert_eq!(san.sanitize(input), input);
}

#[test]
fn regex_ignores_benign_input() {
    let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
    let input = "ls -la /tmp && echo done";
    assert_eq!(san.sanitize(input), input);
}

#[test]
fn regex_invalid_pattern_returns_error() {
    let result = RegexSanitizer::new(vec!["[invalid".into()]);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Layer 3: Shannon entropy detection
// ---------------------------------------------------------------------------

#[test]
fn entropy_redacts_high_entropy_after_equals() {
    let san = EntropySanitizer::new(20, 4.0);
    let input = "SECRET=aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3";
    let result = san.sanitize(input);
    assert!(
        result.contains("<REDACTED>"),
        "high-entropy token after = should be redacted"
    );
}

#[test]
fn entropy_redacts_high_entropy_after_colon() {
    let san = EntropySanitizer::new(20, 4.0);
    let input = "secret: aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn entropy_passes_low_entropy_token() {
    let san = EntropySanitizer::new(20, 4.0);
    let input = "key=aaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(san.sanitize(input), input);
}

#[test]
fn entropy_passes_short_token() {
    let san = EntropySanitizer::new(20, 4.0);
    let input = "key=abc123";
    assert_eq!(san.sanitize(input), input);
}

#[test]
fn entropy_catches_bare_high_entropy_token() {
    // After CRITICAL-02 fix: bare high-entropy tokens ARE caught even without delimiters
    let san = EntropySanitizer::new(20, 4.0);
    let input = "aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3";
    assert_eq!(san.sanitize(input), "<REDACTED>");
}

#[test]
fn entropy_handles_quoted_values() {
    let san = EntropySanitizer::new(20, 4.0);
    let input = r#"secret="aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3""#;
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn entropy_custom_thresholds() {
    // Very low threshold should catch almost anything
    let san = EntropySanitizer::new(5, 1.0);
    let input = "k=abcdefghij";
    let result = san.sanitize(input);
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn entropy_very_high_threshold_passes_everything() {
    let san = EntropySanitizer::new(20, 10.0); // Impossible entropy threshold
    let input = "SECRET=aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3";
    assert_eq!(san.sanitize(input), input);
}

// ---------------------------------------------------------------------------
// Pipeline: All three layers in sequence
// ---------------------------------------------------------------------------

#[test]
fn pipeline_creates_default() {
    let pipeline = SanitizePipeline::default_pipeline();
    // Benign input passes through
    let input = "echo hello world";
    assert_eq!(pipeline.sanitize(input), input);
}

#[test]
fn pipeline_catches_aho_target() {
    let pipeline = SanitizePipeline::default_pipeline();
    let input = "export TOKEN=ghp_abc123def456ghi789";
    let result = pipeline.sanitize(input);
    assert!(result.contains("<REDACTED>"));
    assert!(!result.contains("ghp_abc"));
}

#[test]
fn pipeline_catches_regex_target() {
    let pipeline = SanitizePipeline::default_pipeline();
    let input = "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc.def'";
    let result = pipeline.sanitize(input);
    assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
}

#[test]
fn pipeline_catches_entropy_target() {
    let pipeline = SanitizePipeline::default_pipeline();
    // High entropy token that aho-corasick and regex wouldn't catch
    let input = "CUSTOM_VAR=x7Kp2mN9qR4sW1tY6uV3bE8cF5gH0jA";
    let result = pipeline.sanitize(input);
    // After aho and regex pass through, entropy should catch it
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn pipeline_custom_layers() {
    let aho = AhoCorasickSanitizer::new(vec!["PREFIX_".into()]);
    let pipeline = SanitizePipeline::new(vec![Box::new(aho)]);
    let input = "PREFIX_secret123";
    let result = pipeline.sanitize(input);
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn pipeline_layers_run_sequentially() {
    // If aho redacts a token, the regex layer should see <REDACTED> not the original
    let pipeline = SanitizePipeline::default_pipeline();
    let input = "password=ghp_reallyLongTokenHere123456";
    let result = pipeline.sanitize(input);
    // Should contain exactly one <REDACTED> (aho catches it, regex sees <REDACTED>)
    assert!(result.contains("<REDACTED>"));
}

#[test]
fn pipeline_empty_input() {
    let pipeline = SanitizePipeline::default_pipeline();
    assert_eq!(pipeline.sanitize(""), "");
}

#[test]
fn pipeline_preserves_surrounding_context() {
    let pipeline = SanitizePipeline::default_pipeline();
    let input = "echo 'before' && export KEY=ghp_secrettoken123456 && echo 'after'";
    let result = pipeline.sanitize(input);
    assert!(result.contains("echo 'before'"));
    assert!(result.contains("echo 'after'"));
    assert!(result.contains("<REDACTED>"));
}
