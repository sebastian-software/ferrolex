//! Experimental Node.js binding spike for ferrolex.
//!
//! This crate is deliberately unpublished. It exposes only immutable,
//! plain-word-list checking and bounded suggestions while packaging and API
//! stability are evaluated.

// napi-rs generates Node-API registration glue with unsafe code. The
// handwritten adapter below remains safe Rust; core crates still forbid it.
#![allow(unsafe_code)]

use ferrolex_core::{Dictionary, Normalization, WordList};
use ferrolex_suggest::{SuggestConfig, Suggester};
use napi_derive::napi;

/// Immutable checker backed by newline-delimited plain-word-list text.
#[napi]
pub struct Checker {
    words: WordList,
}

#[napi]
#[allow(
    clippy::needless_pass_by_value,
    reason = "napi-rs converts JavaScript strings into owned Rust values"
)]
impl Checker {
    /// Creates a checker from UTF-8, newline-delimited word-list text.
    #[napi(constructor)]
    #[must_use]
    pub fn new(words: String) -> Self {
        Self {
            words: WordList::from_text(Normalization::Exact, &words),
        }
    }

    /// Returns whether this checker recognizes a word.
    #[napi]
    #[must_use]
    pub fn check(&self, word: String) -> bool {
        self.words.contains(&word)
    }

    /// Returns deterministic bounded spelling suggestions for a word.
    #[napi]
    #[must_use]
    pub fn suggest(&self, word: String) -> Vec<String> {
        Suggester::new(&self.words, SuggestConfig::default())
            .suggest(&word)
            .suggestions()
            .iter()
            .map(|suggestion| suggestion.word().to_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Checker;

    #[test]
    fn exposes_checking_and_suggestions() {
        let checker = Checker::new("ferrolex\nFerris".to_owned());

        assert!(checker.check("ferrolex".to_owned()));
        assert!(!checker.check("ferolex".to_owned()));
        assert_eq!(checker.suggest("ferolex".to_owned()), ["ferrolex"]);
    }
}
