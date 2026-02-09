use aho_corasick::AhoCorasick;

use super::Sanitizer;

/// Layer 1: Literal prefix matching via aho-corasick.
pub struct AhoCorasickSanitizer {
    automaton: AhoCorasick,
    prefixes: Vec<String>,
}

impl AhoCorasickSanitizer {
    /// Build from a list of known secret prefixes.
    pub fn new(prefixes: Vec<String>) -> Self {
        let automaton = AhoCorasick::new(&prefixes).expect("valid aho-corasick patterns");
        Self {
            automaton,
            prefixes,
        }
    }

    /// Default known secret prefixes.
    pub fn default_prefixes() -> Vec<String> {
        vec![
            // Anthropic
            "sk-ant-".into(),
            // OpenAI
            "sk-proj-".into(),
            // GitHub
            "ghp_".into(),
            "gho_".into(),
            "ghs_".into(),
            "github_pat_".into(),
            // AWS
            "AKIA".into(),
            "ASIA".into(),
            // Slack
            "xoxb-".into(),
            "xoxp-".into(),
            "xoxs-".into(),
            "xoxa-".into(),
            // GitLab
            "glpat-".into(),
            "glsa-".into(),
            // Package registries
            "npm_".into(),
            "pypi-".into(),
            // Age encryption
            "AGE-SECRET-KEY-".into(),
            // PEM keys
            "-----BEGIN".into(),
            "PRIVATE KEY".into(),
            // Stripe
            "whsec_".into(),
            "sk_live_".into(),
            "sk_test_".into(),
            "rk_live_".into(),
            "rk_test_".into(),
            // SendGrid
            "SG.".into(),
            // DigitalOcean
            "dop_v1_".into(),
            // New Relic
            "nrk-".into(),
            "NRAK-".into(),
            // Hugging Face
            "hf_".into(),
            // HashiCorp Vault
            "vlt_".into(),
            "hvs.".into(),
            // 1Password
            "op_".into(),
            // Google
            "AIzaSy".into(),
            "ya29.".into(),
        ]
    }
}

impl Sanitizer for AhoCorasickSanitizer {
    fn sanitize(&self, input: &str) -> String {
        if self.prefixes.is_empty() {
            return input.to_string();
        }

        let mut result = input.to_string();
        // Find all matches and collect them in reverse order to replace from end to start.
        let mut matches: Vec<(usize, usize)> = Vec::new();

        for mat in self.automaton.find_iter(input) {
            let start = mat.start();
            // From the prefix match point, extend to the end of the token
            // (non-whitespace, non-quote, non-comma, non-semicolon).
            let rest = &input[start..];
            let token_end = rest
                .find(|c: char| {
                    c.is_whitespace()
                        || c == '"'
                        || c == '\''
                        || c == ','
                        || c == ';'
                        || c == '}'
                        || c == ']'
                        || c == ')'
                        || c == '`'
                        || c == '\n'
                        || c == '\r'
                })
                .unwrap_or(rest.len());
            let end = start + token_end;

            // Only redact if the token is reasonably long (at least the prefix + some chars)
            let prefix_len = mat.end() - mat.start();
            if token_end > prefix_len {
                matches.push((start, end));
            }
        }

        // Deduplicate overlapping ranges and apply replacements in reverse order.
        matches.sort_by(|a, b| a.0.cmp(&b.0));
        let merged = merge_ranges(&matches);

        for &(start, end) in merged.iter().rev() {
            result.replace_range(start..end, "<REDACTED>");
        }

        result
    }

    fn name(&self) -> &str {
        "aho-corasick"
    }
}

/// Merge overlapping or adjacent ranges.
fn merge_ranges(ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut merged = vec![ranges[0]];
    for &(start, end) in &ranges[1..] {
        let last = merged.last_mut().unwrap();
        if start <= last.1 {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_github_token() {
        let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
        let input = "token ghp_abc123def456ghi789";
        let result = san.sanitize(input);
        assert_eq!(result, "token <REDACTED>");
        assert!(!result.contains("ghp_"));
    }

    #[test]
    fn test_redacts_aws_key() {
        let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
        let input = "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
        assert!(!result.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn test_no_match_passes_through() {
        let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
        let input = "echo hello world";
        let result = san.sanitize(input);
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_multiple_secrets() {
        let san = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
        let input = "curl -H ghp_token123 -H xoxb-slack-token";
        let result = san.sanitize(input);
        assert_eq!(result, "curl -H <REDACTED> -H <REDACTED>");
    }
}
