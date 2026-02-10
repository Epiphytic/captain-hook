use regex::RegexSet;

use crate::error::HookwiseError;

use super::Sanitizer;

/// Layer 2: Positional/contextual pattern matching via RegexSet.
pub struct RegexSanitizer {
    regex_set: RegexSet,
    patterns: Vec<regex::Regex>,
}

impl RegexSanitizer {
    /// Build from a list of regex pattern strings.
    pub fn new(patterns: Vec<String>) -> crate::error::Result<Self> {
        let regex_set = RegexSet::new(&patterns).map_err(|e| HookwiseError::InvalidPolicy {
            reason: format!("invalid regex pattern: {e}"),
        })?;
        let compiled: Vec<regex::Regex> = patterns
            .iter()
            .map(|p| {
                regex::Regex::new(p).map_err(|e| HookwiseError::InvalidPolicy {
                    reason: format!("invalid regex pattern: {e}"),
                })
            })
            .collect::<crate::error::Result<Vec<_>>>()?;
        Ok(Self {
            regex_set,
            patterns: compiled,
        })
    }

    /// Default regex patterns for secret detection.
    pub fn default_patterns() -> Vec<String> {
        vec![
            // Bearer tokens
            r"(?i)(bearer\s+)[a-zA-Z0-9_\-\.]{20,}".into(),
            // API key/token/secret/password assignments
            r"(?i)((?:api[_-]?key|token|secret|password|passwd|credentials?)\s*[=:]\s*)\S{8,}"
                .into(),
            // Connection strings with credentials
            r"(?i)((?:postgres|mysql|mongodb|redis|amqp)://\S+?:)\S+?@".into(),
            // CLI password/token flags
            r"(?i)((?:--password|--token|--secret|--api-key|-p)\s+)\S{8,}".into(),
        ]
    }
}

impl Sanitizer for RegexSanitizer {
    fn sanitize(&self, input: &str) -> String {
        // Use the RegexSet for fast matching, then apply individual regexes for replacement.
        let matching: Vec<usize> = self.regex_set.matches(input).into_iter().collect();
        if matching.is_empty() {
            return input.to_string();
        }

        let mut result = input.to_string();
        for &idx in &matching {
            let re = &self.patterns[idx];
            // Replace the secret part (the capture group after the prefix) with <REDACTED>.
            // Each pattern is designed so that group 1 is the prefix to keep.
            result = re
                .replace_all(&result, |caps: &regex::Captures| {
                    if let Some(prefix) = caps.get(1) {
                        format!("{}<REDACTED>", prefix.as_str())
                    } else {
                        "<REDACTED>".to_string()
                    }
                })
                .into_owned();
        }

        result
    }

    fn name(&self) -> &str {
        "regex"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bearer_token() {
        let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
        let input = r#"curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.longtoken""#;
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn test_api_key_assignment() {
        let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
        let input = "api_key=super_secret_key_12345678";
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
        assert!(!result.contains("super_secret_key_12345678"));
    }

    #[test]
    fn test_connection_string() {
        let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
        let input = "postgres://admin:secretpass123@localhost:5432/mydb";
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
        assert!(!result.contains("secretpass123"));
    }

    #[test]
    fn test_cli_password_flag() {
        let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
        let input = "mysql --password mysecretpassword123";
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
        assert!(!result.contains("mysecretpassword123"));
    }

    #[test]
    fn test_no_match() {
        let san = RegexSanitizer::new(RegexSanitizer::default_patterns()).unwrap();
        let input = "echo hello world";
        let result = san.sanitize(input);
        assert_eq!(result, "echo hello world");
    }
}
