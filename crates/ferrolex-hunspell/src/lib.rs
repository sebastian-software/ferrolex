//! Hunspell fetch, import, and conversion APIs.
//!
//! Ferrolex will use Hunspell-compatible dictionaries as source material, then
//! compile them into a Ferrolex-owned runtime format.

use ferrolex_dict::DictionaryMetadata;

/// Hunspell dictionary source descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HunspellSource {
    /// Dictionary metadata and license information.
    pub metadata: DictionaryMetadata,
    /// URL or local path to the `.aff` source.
    pub aff_source: String,
    /// URL or local path to the `.dic` source.
    pub dic_source: String,
}
