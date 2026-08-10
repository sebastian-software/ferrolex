//! Core dictionary interfaces for ferrolex.
//!
//! This crate deliberately has no dependency on any dictionary file format.
//! Importers and compiled dictionary loaders implement [`Dictionary`] at their
//! respective boundaries.

#![forbid(unsafe_code)]

mod composition;
mod lexicon;
mod overlay;

pub use composition::{Checker, CheckerBuilder};
pub use lexicon::{Dictionary, Normalization, WordList, WordListError};
pub use overlay::UserDictionary;
