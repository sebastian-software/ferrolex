//! Dictionary metadata and runtime lookup contracts.

use ferrolex_core::{DictionaryProvider, LocaleCode, Suggestion};

/// Metadata for a dictionary source or compiled dictionary artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DictionaryMetadata {
    /// Locale covered by the dictionary.
    pub locale: LocaleCode,
    /// Human-readable source name.
    pub source_name: String,
    /// License identifier or license summary for this dictionary.
    pub license: String,
    /// Optional upstream URL.
    pub source_url: Option<String>,
}

/// Placeholder runtime dictionary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dictionary {
    metadata: DictionaryMetadata,
}

impl Dictionary {
    /// Creates a dictionary placeholder from metadata.
    #[must_use]
    pub const fn new(metadata: DictionaryMetadata) -> Self {
        Self { metadata }
    }

    /// Returns dictionary metadata.
    #[must_use]
    pub const fn metadata(&self) -> &DictionaryMetadata {
        &self.metadata
    }
}

impl DictionaryProvider for Dictionary {
    fn accepts(&self, _locale: Option<&LocaleCode>, _word: &str) -> bool {
        false
    }

    fn suggest(&self, _locale: Option<&LocaleCode>, _word: &str) -> Vec<Suggestion> {
        Vec::new()
    }
}
