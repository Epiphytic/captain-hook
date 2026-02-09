use super::Sanitizer;

/// Layer 3: Shannon entropy detection for unknown secret formats.
pub struct EntropySanitizer {
    /// Minimum token length to consider. Default: 20.
    pub min_length: usize,
    /// Minimum Shannon entropy to flag. Default: 4.0.
    pub min_entropy: f64,
}

impl EntropySanitizer {
    pub fn new(min_length: usize, min_entropy: f64) -> Self {
        Self {
            min_length,
            min_entropy,
        }
    }

    /// Calculate Shannon entropy of a string.
    fn shannon_entropy(s: &str) -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        let mut freq = [0u32; 256];
        for &b in s.as_bytes() {
            freq[b as usize] += 1;
        }
        let len = s.len() as f64;
        freq.iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / len;
                -p * p.log2()
            })
            .sum()
    }
}

impl Sanitizer for EntropySanitizer {
    fn sanitize(&self, input: &str) -> String {
        // Look for tokens after '=' or ':' that are long and high-entropy.
        let mut result = input.to_string();
        let mut replacements: Vec<(usize, usize)> = Vec::new();

        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Look for '=' or ':' delimiters
            if bytes[i] == b'=' || bytes[i] == b':' {
                i += 1;
                // Skip optional whitespace after delimiter
                while i < len && bytes[i] == b' ' {
                    i += 1;
                }
                // Skip optional quotes
                let in_quote = i < len && (bytes[i] == b'"' || bytes[i] == b'\'');
                let quote_char = if in_quote { bytes[i] } else { 0 };
                if in_quote {
                    i += 1;
                }
                // Collect the token
                let token_start = i;
                if in_quote {
                    while i < len && bytes[i] != quote_char {
                        i += 1;
                    }
                } else {
                    while i < len
                        && !bytes[i].is_ascii_whitespace()
                        && bytes[i] != b','
                        && bytes[i] != b';'
                        && bytes[i] != b'"'
                        && bytes[i] != b'\''
                    {
                        i += 1;
                    }
                }
                let token_end = i;
                let token = &input[token_start..token_end];

                if token.len() >= self.min_length {
                    let entropy = Self::shannon_entropy(token);
                    if entropy > self.min_entropy {
                        replacements.push((token_start, token_end));
                    }
                }
            } else {
                i += 1;
            }
        }

        // Pass 2: Split on whitespace and check ALL tokens >= min_length
        // This catches bare high-entropy tokens without delimiters.
        // Use byte scanning to find exact token positions.
        {
            let bytes = input.as_bytes();
            let len = bytes.len();
            let mut pos = 0;
            while pos < len {
                // Skip whitespace
                if bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                    continue;
                }
                // Find end of non-whitespace token
                let token_start = pos;
                while pos < len && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let token_end = pos;
                let token = &input[token_start..token_end];

                if token.len() >= self.min_length {
                    let entropy = Self::shannon_entropy(token);
                    if entropy > self.min_entropy {
                        // Check it's not already covered by a delimiter-based redaction
                        let already_covered = replacements
                            .iter()
                            .any(|&(s, e)| s <= token_start && token_end <= e);
                        if !already_covered {
                            replacements.push((token_start, token_end));
                        }
                    }
                }
            }
        }

        // Sort and merge overlapping replacements
        replacements.sort_by_key(|&(start, _)| start);
        let merged = merge_ranges(&replacements);

        // Apply replacements in reverse order
        for &(start, end) in merged.iter().rev() {
            result.replace_range(start..end, "<REDACTED>");
        }

        result
    }

    fn name(&self) -> &str {
        "entropy"
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
    fn test_high_entropy_token() {
        let san = EntropySanitizer::new(20, 4.0);
        // A random-looking token with high entropy
        let input = "SECRET_KEY=aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1v";
        let result = san.sanitize(input);
        assert!(result.contains("<REDACTED>"));
    }

    #[test]
    fn test_low_entropy_token_passes() {
        let san = EntropySanitizer::new(20, 4.0);
        // A repetitive token with low entropy
        let input = "key=aaaaaaaaaaaaaaaaaaaa";
        let result = san.sanitize(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_short_token_passes() {
        let san = EntropySanitizer::new(20, 4.0);
        let input = "key=short";
        let result = san.sanitize(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_no_delimiter() {
        let san = EntropySanitizer::new(20, 4.0);
        let input = "echo hello world";
        let result = san.sanitize(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_shannon_entropy_uniform() {
        // 256 unique bytes should give ~8.0 entropy
        let entropy = EntropySanitizer::shannon_entropy("abcdefghijklmnop");
        assert!(entropy > 3.5);
    }

    #[test]
    fn test_shannon_entropy_single_char() {
        let entropy = EntropySanitizer::shannon_entropy("aaaa");
        assert!((entropy - 0.0).abs() < f64::EPSILON);
    }
}
