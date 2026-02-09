pub mod aho;
pub mod encoding;
pub mod entropy;
pub mod regex_san;

/// A single sanitization layer.
pub trait Sanitizer: Send + Sync {
    /// Sanitize the input string, replacing detected secrets with `<REDACTED>`.
    fn sanitize(&self, input: &str) -> String;

    /// Name of this sanitizer layer (for logging/debugging).
    fn name(&self) -> &str;
}

/// The complete sanitization pipeline. Runs all layers in sequence.
pub struct SanitizePipeline {
    layers: Vec<Box<dyn Sanitizer>>,
}

impl SanitizePipeline {
    /// Create the default pipeline with all layers and built-in patterns.
    /// Order: encoding pre-process -> aho-corasick -> regex -> entropy.
    pub fn default_pipeline() -> Self {
        let aho_for_encoding =
            aho::AhoCorasickSanitizer::new(aho::AhoCorasickSanitizer::default_prefixes());
        let regex_for_encoding =
            regex_san::RegexSanitizer::new(regex_san::RegexSanitizer::default_patterns())
                .expect("default regex patterns should compile");
        let encoding_layer = encoding::EncodingSanitizer::new(vec![
            Box::new(aho_for_encoding),
            Box::new(regex_for_encoding),
        ]);

        let aho = aho::AhoCorasickSanitizer::new(aho::AhoCorasickSanitizer::default_prefixes());
        let regex = regex_san::RegexSanitizer::new(regex_san::RegexSanitizer::default_patterns())
            .expect("default regex patterns should compile");
        let entropy = entropy::EntropySanitizer::new(20, 4.0);

        Self {
            layers: vec![
                Box::new(encoding_layer),
                Box::new(aho),
                Box::new(regex),
                Box::new(entropy),
            ],
        }
    }

    /// Create a pipeline from custom layers.
    pub fn new(layers: Vec<Box<dyn Sanitizer>>) -> Self {
        Self { layers }
    }

    /// Run all sanitization layers in sequence.
    pub fn sanitize(&self, input: &str) -> String {
        let mut result = input.to_string();
        for layer in &self.layers {
            result = layer.sanitize(&result);
        }
        result
    }
}
