//! OXC-based JavaScript and TypeScript candidate extraction APIs.
//!
//! This crate is intentionally not tied to Palamedes-specific conventions.

use ferrolex_core::{SourceAdapter, Span};

/// Placeholder OXC adapter.
#[derive(Clone, Debug, Default)]
pub struct OxcAdapter;

impl SourceAdapter for OxcAdapter {
    fn extract_spans(&self, _source: &str) -> Vec<Span> {
        Vec::new()
    }
}
