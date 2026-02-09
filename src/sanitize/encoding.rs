use super::Sanitizer;

/// Pre-processing sanitizer that detects base64 and URL-encoded secrets.
///
/// Before the main aho/regex/entropy pipeline runs, this layer:
/// 1. Detects base64-encoded strings (40+ chars), decodes them, and checks
///    the decoded content against a sub-sanitizer.
/// 2. URL-decodes %XX sequences and re-checks.
///
/// If the decoded content would be redacted by the sub-sanitizer, the
/// original encoded string is redacted.
pub struct EncodingSanitizer {
    /// The sub-sanitizer used to check decoded content.
    /// Typically the aho-corasick + regex layers (not entropy, to avoid false positives).
    inner: Vec<Box<dyn Sanitizer>>,
}

impl EncodingSanitizer {
    pub fn new(inner: Vec<Box<dyn Sanitizer>>) -> Self {
        Self { inner }
    }

    /// Check if decoded content would be flagged by any inner sanitizer.
    fn would_redact(&self, decoded: &str) -> bool {
        for sanitizer in &self.inner {
            let result = sanitizer.sanitize(decoded);
            if result != decoded {
                return true;
            }
        }
        false
    }

    /// Detect and check base64 strings in the input.
    /// Looks for tokens of 40+ chars matching [A-Za-z0-9+/=].
    fn check_base64(&self, input: &str) -> Vec<(usize, usize)> {
        let mut redactions = Vec::new();
        let bytes = input.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Find start of a potential base64 token
            if is_base64_char(bytes[i]) {
                let start = i;
                while i < len && is_base64_char(bytes[i]) {
                    i += 1;
                }
                let token = &input[start..i];
                if token.len() >= 40 {
                    // Attempt base64 decode
                    if let Some(decoded_bytes) = base64_decode(token) {
                        if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                            if self.would_redact(&decoded_str) {
                                redactions.push((start, i));
                            }
                        }
                    }
                }
            } else {
                i += 1;
            }
        }

        redactions
    }

    /// Find spans in the input that contain URL-encoded secret prefixes.
    /// We re-check token-by-token to only redact the relevant tokens.
    fn find_url_encoded_spans(&self, input: &str) -> Vec<(usize, usize)> {
        let mut redactions = Vec::new();

        // Split on whitespace and check each token
        let mut offset = 0;
        for segment in input.split_whitespace() {
            let seg_start = input[offset..].find(segment).unwrap_or(0) + offset;
            let seg_end = seg_start + segment.len();

            if segment.contains('%') {
                let decoded = url_decode(segment);
                if decoded != segment && self.would_redact(&decoded) {
                    redactions.push((seg_start, seg_end));
                }
            }

            offset = seg_end;
        }

        redactions
    }
}

impl Sanitizer for EncodingSanitizer {
    fn sanitize(&self, input: &str) -> String {
        let mut result = input.to_string();
        let mut all_redactions = Vec::new();

        // Pass 1: Check base64-encoded tokens
        all_redactions.extend(self.check_base64(input));

        // Pass 2: Check URL-encoded tokens
        all_redactions.extend(self.find_url_encoded_spans(input));

        // Sort and merge overlapping ranges
        all_redactions.sort_by_key(|&(start, _)| start);
        let merged = merge_ranges(&all_redactions);

        // Apply redactions in reverse order
        for &(start, end) in merged.iter().rev() {
            result.replace_range(start..end, "<REDACTED>");
        }

        result
    }

    fn name(&self) -> &str {
        "encoding"
    }
}

fn is_base64_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
}

/// Simple base64 decoder (standard alphabet with padding).
/// Returns None if the input is not valid base64.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Strip padding for length check
    let stripped = input.trim_end_matches('=');

    // Each base64 char encodes 6 bits; we need groups of 4 chars -> 3 bytes
    // Validate all chars are valid base64
    if !stripped
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    {
        return None;
    }

    // Use a simple lookup table approach
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let bytes = input.as_bytes();
    let mut i = 0;

    while i + 3 < bytes.len() {
        let a = b64_val(bytes[i])?;
        let b = b64_val(bytes[i + 1])?;
        let c = if bytes[i + 2] == b'=' {
            0
        } else {
            b64_val(bytes[i + 2])?
        };
        let d = if bytes[i + 3] == b'=' {
            0
        } else {
            b64_val(bytes[i + 3])?
        };

        output.push((a << 2) | (b >> 4));
        if bytes[i + 2] != b'=' {
            output.push((b << 4) | (c >> 2));
        }
        if bytes[i + 3] != b'=' {
            output.push((c << 6) | d);
        }

        i += 4;
    }

    Some(output)
}

fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        b'=' => Some(0),
        _ => None,
    }
}

/// URL-decode %XX sequences.
fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'%' && i + 2 < len {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push((hi << 4 | lo) as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
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
    use crate::sanitize::aho::AhoCorasickSanitizer;

    fn make_encoding_sanitizer() -> EncodingSanitizer {
        let aho = AhoCorasickSanitizer::new(AhoCorasickSanitizer::default_prefixes());
        EncodingSanitizer::new(vec![Box::new(aho)])
    }

    #[test]
    fn test_base64_encoded_secret_detected() {
        let san = make_encoding_sanitizer();
        // "sk-ant-123456789012345678901234" base64-encoded
        // sk-ant-123456789012345678901234 -> c2stYW50LTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNA==
        let input = "echo c2stYW50LTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNA== | base64 -d";
        let result = san.sanitize(input);
        assert!(
            result.contains("<REDACTED>"),
            "should redact base64-encoded secret, got: {}",
            result
        );
    }

    #[test]
    fn test_url_encoded_prefix() {
        let san = make_encoding_sanitizer();
        // sk-ant- URL-encoded: sk%2Dant%2D
        let input = "export KEY=sk%2Dant%2Dsecret123456789";
        let result = san.sanitize(input);
        assert!(
            result.contains("<REDACTED>"),
            "should redact URL-encoded secret prefix, got: {}",
            result
        );
    }

    #[test]
    fn test_normal_base64_not_redacted() {
        let san = make_encoding_sanitizer();
        // Normal base64 that doesn't decode to a secret
        let input = "echo aGVsbG8gd29ybGQgdGhpcyBpcyBhIG5vcm1hbCBiYXNlNjQgc3RyaW5n | base64 -d";
        let result = san.sanitize(input);
        assert!(!result.contains("<REDACTED>"));
    }

    #[test]
    fn test_no_encoding_passes_through() {
        let san = make_encoding_sanitizer();
        let input = "echo hello world";
        let result = san.sanitize(input);
        assert_eq!(result, input);
    }
}
