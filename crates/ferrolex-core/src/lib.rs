//! Core dictionary interfaces for ferrolex.
//!
//! This crate deliberately has no dependency on any dictionary file format.
//! Importers and compiled dictionary loaders implement [`Dictionary`] at their
//! respective boundaries.

#![forbid(unsafe_code)]

mod checker;
mod word_list;

pub use checker::{Checker, CheckerBuilder};
pub use word_list::{Dictionary, Normalization, WordList, WordListError};
