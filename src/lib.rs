//! Public umbrella crate for ferrolex.
//!
//! The workspace keeps focused crates for import, compilation, suggestions,
//! and integration adapters. This package is the one-product release boundary
//! and re-exports the stable core dictionary API.
//!
//! ## Stability
//!
//! This is a **supported public Rust API** under ferrolex's [pre-1.0 release
//! contract](https://github.com/sebastian-software/ferrolex/blob/main/docs/release-contract.md).
//! The optional `ffi` feature is **experimental**: it enables the unpublished
//! `ferrolex-ffi/c-abi` spike and is not part of this crate's supported API.
//!
//! ```
//! use ferrolex::{Dictionary, WordList};
//!
//! let dictionary = WordList::new(["ferrolex"])?;
//! assert!(dictionary.contains("ferrolex"));
//! # Ok::<(), ferrolex::WordListError>(())
//! ```

#![forbid(unsafe_code)]

pub use ferrolex_core::*;
