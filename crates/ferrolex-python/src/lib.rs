//! Experimental Python binding spike for ferrolex.
//!
//! The accompanying pyproject configuration makes this crate buildable with
//! maturin, but no Python wheel or crates.io package is published from this
//! spike.
//!
//! This is an **experimental**, unpublished (`publish = false`) binding under
//! ferrolex's [pre-1.0 release
//! contract](https://github.com/sebastian-software/ferrolex/blob/main/docs/release-contract.md).

#![forbid(unsafe_code)]

use ferrolex_core::{Dictionary, Normalization, WordList};
use ferrolex_suggest::{SuggestConfig, Suggester};
use pyo3::prelude::*;

/// Immutable checker backed by newline-delimited plain-word-list text.
#[pyclass]
pub struct Checker {
    words: WordList,
}

#[pymethods]
#[allow(
    clippy::needless_pass_by_value,
    reason = "PyO3 converts Python strings into owned Rust values"
)]
impl Checker {
    /// Creates a checker from UTF-8, newline-delimited word-list text.
    #[new]
    #[must_use]
    fn new(words: String) -> Self {
        Self {
            words: WordList::from_text(Normalization::Exact, &words),
        }
    }

    /// Returns whether this checker recognizes a word.
    #[must_use]
    fn check(&self, word: String) -> bool {
        self.words.contains(&word)
    }

    /// Returns deterministic bounded spelling suggestions for a word.
    #[must_use]
    fn suggest(&self, word: String) -> Vec<String> {
        Suggester::new(&self.words, SuggestConfig::default())
            .suggest(&word)
            .suggestions()
            .iter()
            .map(|suggestion| suggestion.word().to_owned())
            .collect()
    }
}

/// ferrolex Python extension module.
#[pymodule]
fn ferrolex_python(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Checker>()?;
    Ok(())
}
