//! Core dictionary interfaces for ferrolex.
//!
//! This crate deliberately has no dependency on any dictionary file format.
//! Importers and compiled dictionary loaders implement [`Dictionary`] at their
//! respective boundaries.
//!
//! ```
//! use ferrolex_core::{Dictionary, WordList};
//!
//! let dictionary = WordList::new(["word"])?;
//! assert!(dictionary.contains("word"));
//! # Ok::<(), ferrolex_core::WordListError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod composition;
mod lexicon;
mod overlay;

pub use composition::{Checker, CheckerBuilder};
pub use lexicon::{Dictionary, Normalization, WordList, WordListError};
pub use overlay::UserDictionary;
