//! Public umbrella crate for ferrolex.
//!
//! The workspace keeps focused crates for import, compilation, suggestions,
//! and integration adapters. This package is the one-product release boundary
//! and exposes the supported product crates through one version-locked API.
//!
//! ```
//! use ferrolex::{import, Dictionary, ImportMode, SuggestConfig};
//!
//! let imported = import(
//!     "example.aff",
//!     "SET UTF-8\n",
//!     "example.dic",
//!     "1\nferrolex\n",
//!     ImportMode::Strict,
//! )?;
//! let dictionary = imported.dictionary();
//! assert!(dictionary.contains("ferrolex"));
//! assert_eq!(
//!     dictionary
//!         .suggester(SuggestConfig::default())
//!         .suggest("ferolex")
//!         .suggestions()[0]
//!         .word(),
//!     "ferrolex"
//! );
//! # Ok::<(), ferrolex::ImportError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub use ferrolex_core::*;

/// Verified dictionary catalogs and caller-controlled installers.
pub use ferrolex_dictionaries as dictionaries;
/// Hunspell-compatible import and runtime dictionary APIs.
pub use ferrolex_hunspell as hunspell;
/// Deterministic, bounded suggestion APIs.
pub use ferrolex_suggest as suggest;

pub use ferrolex_dictionaries::{
    find_locale, DictionaryInstaller, FetchError, Fetcher, InstalledDictionary,
    LibreOfficeDictionary, ManifestError, SourceEncoding, UreqFetcher, VerifiedDictionary,
    VerifiedFile, LIBREOFFICE_CATALOG, LIBREOFFICE_REVISION,
};
pub use ferrolex_hunspell::{
    import, import_bytes, import_bytes_with_encodings, ByteEncoding, ByteImportEncodings,
    Diagnostic, DictionaryIr, HunspellDictionary, ImportError, ImportMode, ImportResult, Severity,
};
pub use ferrolex_suggest::{
    CandidateSource, Completeness, RankingSignals, ReplacementRule, SuggestConfig, SuggestScratch,
    Suggester, Suggestion, SuggestionResult,
};
