//! Tokenization and identifier splitting APIs.
//!
//! This crate will own Unicode word segmentation and code identifier splitting
//! such as `camelCase`, `PascalCase`, `snake_case`, and acronym boundaries.

use ferrolex_core::Token;

/// Configuration for Ferrolex tokenization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenizerConfig {
    /// Whether code-style identifiers should be split into word candidates.
    pub split_identifiers: bool,
}

/// Tokenizer entry point.
#[derive(Clone, Debug, Default)]
pub struct Tokenizer {
    config: TokenizerConfig,
}

impl Tokenizer {
    /// Creates a tokenizer from configuration.
    #[must_use]
    pub const fn new(config: TokenizerConfig) -> Self {
        Self { config }
    }

    /// Returns this tokenizer's configuration.
    #[must_use]
    pub const fn config(&self) -> &TokenizerConfig {
        &self.config
    }

    /// Tokenizes a span of text.
    ///
    /// This is intentionally a placeholder while the repository structure and
    /// tokenizer contract settle.
    #[must_use]
    pub fn tokenize(&self, _text: &str) -> Vec<Token> {
        Vec::new()
    }
}
