//! Core dictionary interfaces for ferrolex.
//!
//! This crate deliberately has no dependency on any dictionary file format.
//! Importers and compiled dictionary loaders implement [`Dictionary`] at their
//! respective boundaries.
//!
//! ## Stability
//!
//! This is a **supported public Rust API** under ferrolex's [pre-1.0 release
//! contract](https://github.com/sebastian-software/ferrolex/blob/main/docs/release-contract.md).
//!
//! ```
//! use ferrolex_core::{Dictionary, WordList};
//!
//! let dictionary = WordList::new(["word"])?;
//! assert!(dictionary.contains("word"));
//! # Ok::<(), ferrolex_core::WordListError>(())
//! ```

#![forbid(unsafe_code)]

mod composition;
mod lexicon;
mod overlay;

pub use composition::{Checker, CheckerBuilder};
pub use lexicon::{Dictionary, Normalization, WordList, WordListError};
pub use overlay::UserDictionary;
