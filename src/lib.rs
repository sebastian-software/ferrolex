//! Public umbrella crate for ferrolex.
//!
//! The workspace keeps focused crates for import, compilation, suggestions,
//! and integration adapters. This package is the one-product release boundary
//! and re-exports the stable core dictionary API.

#![forbid(unsafe_code)]

pub use ferrolex_core::*;
