//! Public umbrella crate for ferrolex.
//!
//! The workspace keeps focused crates for import, compilation, suggestions,
//! and integration adapters. This package is the one-product release boundary
//! and re-exports the stable core dictionary API.
//!
//! ```
//! use ferrolex::{Dictionary, WordList};
//!
//! let dictionary = WordList::new(["ferrolex"])?;
//! assert!(dictionary.contains("ferrolex"));
//! # Ok::<(), ferrolex::WordListError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use ferrolex_core::*;
