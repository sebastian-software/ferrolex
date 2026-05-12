//! JSON source candidate extraction APIs.

use ferrolex_core::{SourceAdapter, Span};

/// Placeholder JSON adapter.
#[derive(Clone, Debug, Default)]
pub struct JsonAdapter;

impl SourceAdapter for JsonAdapter {
    fn extract_spans(&self, _source: &str) -> Vec<Span> {
        Vec::new()
    }
}
