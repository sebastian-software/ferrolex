//! Node.js binding for the focused ferrolex spell-checking API.
//!
//! The binding exposes the Rust engine's explicit dictionary, normalization,
//! bounded-suggestion, and user-overlay policies without conflating them with
//! the core crate's dictionary-composition type.

// napi-rs generates Node-API registration glue with unsafe code. The
// handwritten adapter below remains safe Rust; core crates still forbid it.
#![allow(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferrolex_core::{CandidateSource, Dictionary, Normalization, UserDictionary, WordList};
use ferrolex_dictionaries::{
    DictionaryInstaller, LIBREOFFICE_CATALOG, SourceEncoding, UreqFetcher, find_locale,
};
use ferrolex_hunspell::{
    ByteEncoding, ByteImportEncodings, HunspellDictionary, ImportError, ImportMode, ImportResult,
    import_bytes, import_bytes_with_encodings, load_runtime_artifact,
};
use ferrolex_suggest::{Completeness, SuggestConfig, Suggester};
use napi::bindgen_prelude::{AsyncTask, Buffer};
use napi::{Env, Error, Result, Status, Task};
use napi_derive::napi;

/// Unicode normalization applied to word-list, query, and user-word input.
#[napi(string_enum = "camelCase")]
#[derive(Clone, Copy, Debug, Default)]
pub enum NormalizationMode {
    /// Compare UTF-8 strings exactly as supplied.
    #[default]
    Exact,
    /// Canonically normalize Unicode text according to NFC.
    Nfc,
    /// Compatibility-normalize Unicode text according to NFKC.
    Nfkc,
}

impl From<NormalizationMode> for Normalization {
    fn from(mode: NormalizationMode) -> Self {
        match mode {
            NormalizationMode::Exact => Self::Exact,
            NormalizationMode::Nfc => Self::Nfc,
            NormalizationMode::Nfkc => Self::Nfkc,
        }
    }
}

/// Construction options shared by word-list, Hunspell, and runtime-artifact
/// loaders.
#[napi(object)]
#[derive(Clone, Default)]
pub struct CheckerOptions {
    /// Explicit Unicode normalization policy. Defaults to `exact`.
    pub normalization: Option<NormalizationMode>,
}

/// Per-call bounds for deterministic suggestions.
#[napi(object)]
#[derive(Clone, Default)]
pub struct SuggestionOptions {
    /// Maximum number of returned suggestions. Defaults to 8.
    pub max_results: Option<u32>,
    /// Maximum OSA edit distance. Defaults to 2.
    pub max_edit_distance: Option<u32>,
    /// Maximum Unicode scalar values in a query or candidate. Defaults to 64.
    pub max_word_scalars: Option<u32>,
    /// Maximum source candidates inspected. Defaults to 100,000.
    pub max_candidates: Option<u32>,
    /// Maximum dynamic-programming cells evaluated. Defaults to 1,000,000.
    pub max_edit_cells: Option<u32>,
}

impl SuggestionOptions {
    fn config(&self) -> SuggestConfig {
        let defaults = SuggestConfig::default();
        SuggestConfig {
            max_results: self.max_results.map_or(defaults.max_results, as_usize),
            max_edit_distance: self
                .max_edit_distance
                .map_or(defaults.max_edit_distance, as_usize),
            max_word_scalars: self
                .max_word_scalars
                .map_or(defaults.max_word_scalars, as_usize),
            max_candidates: self
                .max_candidates
                .map_or(defaults.max_candidates, as_usize),
            max_edit_cells: self
                .max_edit_cells
                .map_or(defaults.max_edit_cells, as_usize),
        }
    }
}

/// Completeness state for a bounded suggestion request.
#[napi(string_enum = "camelCase")]
#[derive(Clone, Copy, Debug)]
pub enum SuggestionCompleteness {
    /// Every candidate permitted by the configuration was considered.
    Complete,
    /// The configured candidate count was reached before the search completed.
    CandidateLimitReached,
    /// The configured edit-distance work budget was exhausted.
    EditBudgetReached,
    /// The query exceeded the configured maximum length.
    QueryTooLong,
    /// A related seed exceeded the bounded normalization length.
    RelatedSeedTooLong,
}

impl From<Completeness> for SuggestionCompleteness {
    fn from(completeness: Completeness) -> Self {
        match completeness {
            Completeness::Complete => Self::Complete,
            Completeness::CandidateLimitReached => Self::CandidateLimitReached,
            Completeness::EditBudgetReached => Self::EditBudgetReached,
            Completeness::QueryTooLong => Self::QueryTooLong,
            Completeness::RelatedSeedTooLong => Self::RelatedSeedTooLong,
        }
    }
}

/// One ranked spelling suggestion.
#[napi(object)]
pub struct Suggestion {
    /// Display spelling.
    pub word: String,
    /// OSA edit distance from the query, or zero for an explicit replacement.
    pub distance: u32,
}

/// Bounded suggestion output and its completeness signal.
#[napi(object)]
pub struct SuggestionResult {
    /// Suggestions in deterministic rank order.
    pub suggestions: Vec<Suggestion>,
    /// Whether configured candidate and work limits were exhausted.
    pub completeness: SuggestionCompleteness,
}

enum CheckerBackend {
    WordList(WordList),
    Hunspell(Box<HunspellDictionary>),
}

impl CheckerBackend {
    fn dictionary(&self) -> &dyn Dictionary {
        match self {
            Self::WordList(dictionary) => dictionary,
            Self::Hunspell(dictionary) => dictionary.as_ref(),
        }
    }

    fn candidate_source(&self) -> &dyn CandidateSource {
        match self {
            Self::WordList(dictionary) => dictionary,
            Self::Hunspell(dictionary) => dictionary.as_ref(),
        }
    }
}

/// Candidate source combining the immutable engine dictionary and the
/// immediately mutable user-word overlay.
struct CombinedCandidateSource<'source> {
    base: &'source dyn CandidateSource,
    user: &'source UserDictionary,
}

impl CandidateSource for CombinedCandidateSource<'_> {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        let mut keep_going = true;
        self.base.visit_candidates(&mut |candidate| {
            keep_going = visitor(candidate);
            keep_going
        });
        if keep_going {
            self.user.visit_candidates(visitor);
        }
    }

    fn contains_candidate(&self, word: &str) -> bool {
        self.base.contains_candidate(word) || self.user.contains_candidate(word)
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        let mut keep_going = true;
        self.base.visit_nearby_candidates(
            query,
            max_edit_distance,
            max_word_scalars,
            &mut |candidate| {
                keep_going = visitor(candidate);
                keep_going
            },
        );
        if keep_going {
            self.user
                .visit_nearby_candidates(query, max_edit_distance, max_word_scalars, visitor);
        }
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        self.base.is_suggestion_candidate(candidate) || self.user.is_suggestion_candidate(candidate)
    }

    fn candidate_frequency(&self, candidate: &str) -> Option<u64> {
        self.base.candidate_frequency(candidate)
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.base
            .visit_related_candidates(query, seed, max_edit_distance, visitor);
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        self.base.visit_related_seeds(visitor);
    }
}

/// Node.js spell checker backed by a word list, Hunspell dictionary, or
/// validated runtime artifact.
#[napi]
pub struct SpellChecker {
    backend: CheckerBackend,
    normalization: Normalization,
    user_dictionary: Arc<UserDictionary>,
}

#[napi]
#[allow(
    clippy::needless_pass_by_value,
    reason = "napi-rs converts JavaScript strings and options into owned Rust values"
)]
impl SpellChecker {
    /// Creates a spell checker from UTF-8, newline-delimited word-list text.
    ///
    /// Blank lines and lines whose first non-whitespace character is `#` are
    /// ignored. Leading and trailing line whitespace is removed; internal
    /// whitespace and inline `#` characters remain part of a word.
    #[napi(constructor)]
    #[must_use]
    pub fn new(words: String, options: Option<CheckerOptions>) -> Self {
        let normalization = normalization_from_options(options.as_ref());
        Self::from_backend(
            CheckerBackend::WordList(WordList::from_text(normalization, &words)),
            normalization,
        )
    }

    /// Creates a checker by strictly importing caller-owned Hunspell files.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error when either file cannot be read or strict
    /// import reports an unsupported or malformed recognition construct.
    #[napi(factory)]
    pub fn from_hunspell(
        aff_path: String,
        dic_path: String,
        options: Option<CheckerOptions>,
    ) -> Result<Self> {
        let normalization = normalization_from_options(options.as_ref());
        load_hunspell(Path::new(&aff_path), Path::new(&dic_path), None)
            .map(|dictionary| {
                Self::from_backend(
                    CheckerBackend::Hunspell(Box::new(dictionary)),
                    normalization,
                )
            })
            .map_err(node_error)
    }

    /// Loads a standalone, checksummed Hunspell runtime artifact from disk.
    ///
    /// The artifact carries its source provenance and runtime-semantics
    /// version. Invalid, stale, truncated, or checksum-mismatched bytes are
    /// returned as JavaScript errors rather than being silently accepted.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or the artifact fails
    /// runtime-format, checksum, or provenance validation.
    #[napi(factory)]
    pub fn from_runtime_artifact(path: String, options: Option<CheckerOptions>) -> Result<Self> {
        let bytes = fs::read(&path)
            .map_err(|error| node_error(format!("could not read {path}: {error}")))?;
        Self::from_runtime_artifact_bytes(Buffer::from(bytes), options)
    }

    /// Loads a standalone, checksummed Hunspell runtime artifact from bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes fail runtime-format, checksum, or
    /// provenance validation.
    #[napi(factory)]
    pub fn from_runtime_artifact_bytes(
        bytes: Buffer,
        options: Option<CheckerOptions>,
    ) -> Result<Self> {
        let normalization = normalization_from_options(options.as_ref());
        load_runtime_artifact(bytes.as_ref())
            .map(|dictionary| {
                Self::from_backend(
                    CheckerBackend::Hunspell(Box::new(dictionary)),
                    normalization,
                )
            })
            .map_err(node_error)
    }

    /// Installs and strictly imports a digest-pinned catalog dictionary.
    ///
    /// Network, verification, and import work runs outside the JavaScript
    /// event loop. The cache root is always selected by the caller.
    #[napi(ts_return_type = "Promise<SpellChecker>")]
    #[must_use]
    pub fn install(
        locale: String,
        cache_root: String,
        options: Option<CheckerOptions>,
    ) -> AsyncTask<InstallDictionary> {
        AsyncTask::new(InstallDictionary {
            locale,
            cache_root: PathBuf::from(cache_root),
            normalization: normalization_from_options(options.as_ref()),
        })
    }

    /// Returns whether this checker recognizes a word.
    #[napi]
    #[must_use]
    pub fn check(&self, word: String) -> bool {
        let normalized = self.normalization.normalize(&word);
        self.backend.dictionary().contains(normalized.as_ref())
            || self.user_dictionary.contains(&word)
    }

    /// Returns deterministic bounded spelling suggestions and completeness.
    #[napi]
    #[must_use]
    pub fn suggest(&self, word: String, options: Option<SuggestionOptions>) -> SuggestionResult {
        let normalized = self.normalization.normalize(&word);
        let source = CombinedCandidateSource {
            base: self.backend.candidate_source(),
            user: &self.user_dictionary,
        };
        let suggester = Suggester::new(
            &source,
            options
                .as_ref()
                .map_or_else(SuggestConfig::default, SuggestionOptions::config),
        );
        let result = match &self.backend {
            CheckerBackend::WordList(_) => suggester.suggest(normalized.as_ref()),
            CheckerBackend::Hunspell(dictionary) => suggester
                .with_replacement_rules(dictionary.replacement_rules())
                .with_ranking_signals(dictionary.ranking_signals())
                .suggest(normalized.as_ref()),
        };
        SuggestionResult {
            suggestions: result
                .suggestions()
                .iter()
                .map(|suggestion| Suggestion {
                    word: suggestion.word().to_owned(),
                    distance: u32::try_from(suggestion.distance()).unwrap_or(u32::MAX),
                })
                .collect(),
            completeness: result.completeness().into(),
        }
    }

    /// Adds one word to the live user-dictionary overlay.
    ///
    /// Returns whether the word was newly added. Blank lines, comment entries,
    /// and entries containing line breaks are rejected with a JavaScript error.
    ///
    /// # Errors
    ///
    /// Returns an error when the word is empty, a comment entry, or cannot be
    /// represented by the overlay's line-oriented persistence format.
    #[napi]
    pub fn add_user_word(&self, word: String) -> Result<bool> {
        self.user_dictionary.insert(&word).map_err(node_error)
    }

    /// Removes one word from the live user-dictionary overlay.
    ///
    /// # Errors
    ///
    /// Returns an error when the word is empty, a comment entry, or contains a
    /// line break.
    #[napi]
    pub fn remove_user_word(&self, word: String) -> Result<bool> {
        self.user_dictionary.remove(&word).map_err(node_error)
    }

    /// Returns the current user-dictionary words in deterministic order.
    #[napi]
    #[must_use]
    pub fn user_words(&self) -> Vec<String> {
        self.user_dictionary.snapshot()
    }
}

impl SpellChecker {
    fn from_backend(backend: CheckerBackend, normalization: Normalization) -> Self {
        Self {
            backend,
            normalization,
            user_dictionary: Arc::new(UserDictionary::new(normalization)),
        }
    }
}

/// Reviewed source metadata for one managed dictionary.
#[napi(object)]
pub struct CatalogDictionary {
    /// Locale identifier accepted by `SpellChecker.install`.
    pub locale: String,
    /// Pinned upstream revision.
    pub revision: String,
    /// Reviewed SPDX expression for the dictionary data.
    pub license: String,
    /// Immutable upstream license-notice URL.
    pub license_notice_url: String,
}

/// Returns the digest-pinned managed dictionary catalog.
#[napi]
#[must_use]
pub fn dictionary_catalog() -> Vec<CatalogDictionary> {
    LIBREOFFICE_CATALOG
        .into_iter()
        .map(|source| CatalogDictionary {
            locale: source.locale().to_owned(),
            revision: source.revision().to_owned(),
            license: source.license_spdx_expression().to_owned(),
            license_notice_url: source.license_notice_url(),
        })
        .collect()
}

/// Background work for a managed dictionary installation.
pub struct InstallDictionary {
    locale: String,
    cache_root: PathBuf,
    normalization: Normalization,
}

impl Task for InstallDictionary {
    type Output = SpellChecker;
    type JsValue = SpellChecker;

    fn compute(&mut self) -> Result<Self::Output> {
        let source = find_locale(&self.locale).ok_or_else(|| {
            node_error(format!(
                "unsupported managed dictionary locale `{}`",
                self.locale
            ))
        })?;
        let manifest = source.manifest().map_err(node_error)?;
        let installed = DictionaryInstaller::new(UreqFetcher)
            .install(&manifest, &self.cache_root)
            .map_err(node_error)?;
        load_hunspell(
            installed.aff_path(),
            installed.dic_path(),
            catalog_encodings(source.encoding()),
        )
        .map(|dictionary| {
            SpellChecker::from_backend(
                CheckerBackend::Hunspell(Box::new(dictionary)),
                self.normalization,
            )
        })
        .map_err(node_error)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

fn normalization_from_options(options: Option<&CheckerOptions>) -> Normalization {
    options
        .and_then(|options| options.normalization)
        .map_or(Normalization::Exact, Into::into)
}

fn as_usize(value: u32) -> usize {
    usize::try_from(value).expect("u32 fits usize on supported Node targets")
}

fn load_hunspell(
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> std::result::Result<HunspellDictionary, String> {
    let aff_bytes = fs::read(aff_path)
        .map_err(|error| format!("could not read {}: {error}", aff_path.display()))?;
    let dic_bytes = fs::read(dic_path)
        .map_err(|error| format!("could not read {}: {error}", dic_path.display()))?;
    let aff_source = aff_path.display().to_string();
    let dic_source = dic_path.display().to_string();
    let imported = match encodings {
        Some(encodings) => import_bytes_with_encodings(
            &aff_source,
            &aff_bytes,
            &dic_source,
            &dic_bytes,
            encodings,
            ImportMode::Strict,
        ),
        None => import_bytes(
            &aff_source,
            &aff_bytes,
            &dic_source,
            &dic_bytes,
            ImportMode::Strict,
        ),
    };
    imported
        .map(ImportResult::into_dictionary)
        .map_err(|error| format_import_error(&error))
}

fn format_import_error(error: &ImportError) -> String {
    error
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}: {}[{}]: {}",
                diagnostic.source(),
                diagnostic.line(),
                match diagnostic.severity() {
                    ferrolex_hunspell::Severity::Error => "error",
                    ferrolex_hunspell::Severity::Warning => "warning",
                },
                diagnostic.directive(),
                diagnostic.message()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const fn catalog_encodings(encoding: SourceEncoding) -> Option<ByteImportEncodings> {
    match encoding {
        SourceEncoding::Iso8859_1 => Some(ByteImportEncodings::same(ByteEncoding::Iso8859_1)),
        SourceEncoding::Iso8859_2 => Some(ByteImportEncodings::same(ByteEncoding::Iso8859_2)),
        SourceEncoding::MixedUtf8AndIso8859_1 => Some(ByteImportEncodings::new(
            ByteEncoding::Iso8859_1,
            ByteEncoding::Utf8,
        )),
        SourceEncoding::MixedUtf8AndIso8859_2Fallback => Some(ByteImportEncodings::new(
            ByteEncoding::Utf8WithIso8859_2Fallback,
            ByteEncoding::Utf8,
        )),
        SourceEncoding::Utf8 => None,
    }
}

fn node_error(error: impl std::fmt::Display) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ferrolex_hunspell::{ImportMode, SourceDigests, compile_runtime_artifact, import_bytes};
    use napi::bindgen_prelude::Buffer;

    use super::{
        CheckerOptions, NormalizationMode, SpellChecker, SuggestionCompleteness, SuggestionOptions,
        dictionary_catalog,
    };

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn exposes_word_list_checking_and_suggestions() {
        let checker = SpellChecker::new("ferrolex\nFerris".to_owned(), None);

        assert!(checker.check("ferrolex".to_owned()));
        assert!(!checker.check("ferolex".to_owned()));
        let result = checker.suggest("ferolex".to_owned(), None);
        assert_eq!(result.suggestions[0].word, "ferrolex");
        assert_eq!(result.suggestions[0].distance, 1);
        assert!(matches!(
            result.completeness,
            SuggestionCompleteness::Complete
        ));
    }

    #[test]
    fn normalization_and_user_words_are_explicit() {
        let exact = SpellChecker::new("café\n".to_owned(), None);
        assert!(!exact.check("cafe\u{301}".to_owned()));

        let checker = SpellChecker::new(
            "café\n".to_owned(),
            Some(CheckerOptions {
                normalization: Some(NormalizationMode::Nfc),
            }),
        );
        assert!(checker.check("cafe\u{301}".to_owned()));
        assert!(!checker.check("projectword".to_owned()));
        assert!(
            checker
                .add_user_word("projectword".to_owned())
                .expect("word is valid")
        );
        assert!(checker.check("projectword".to_owned()));
        assert_eq!(checker.user_words(), ["projectword"]);
        assert!(
            checker
                .remove_user_word("projectword".to_owned())
                .expect("word is valid")
        );
        assert!(!checker.check("projectword".to_owned()));
    }

    #[test]
    fn suggestion_options_expose_bounded_completeness() {
        let checker = SpellChecker::new("ferrolex\nferret\nforest".to_owned(), None);
        let result = checker.suggest(
            "ferolex".to_owned(),
            Some(SuggestionOptions {
                max_results: Some(1),
                max_edit_distance: Some(2),
                max_word_scalars: Some(64),
                max_candidates: Some(1),
                max_edit_cells: Some(1_000_000),
            }),
        );

        assert_eq!(result.suggestions.len(), 1);
        assert!(matches!(
            result.completeness,
            SuggestionCompleteness::CandidateLimitReached | SuggestionCompleteness::Complete
        ));
    }

    #[test]
    fn strictly_imports_hunspell_and_uses_its_suggestion_signals() {
        let directory = std::env::temp_dir().join(format!(
            "ferrolex-node-hunspell-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).expect("fixture directory is created");
        let aff_path = directory.join("test.aff");
        let dic_path = directory.join("test.dic");
        fs::write(
            &aff_path,
            "SET UTF-8\nREP 1\nREP recieve receive\nSFX S Y 1\nSFX S 0 s .\n",
        )
        .expect("affix fixture is written");
        fs::write(&dic_path, "2\nreceive/S\nferrolex\n").expect("dictionary fixture is written");

        let checker = SpellChecker::from_hunspell(
            aff_path.to_string_lossy().into_owned(),
            dic_path.to_string_lossy().into_owned(),
            None,
        )
        .expect("fixture imports strictly");
        assert!(checker.check("receives".to_owned()));
        assert_eq!(
            checker.suggest("recieve".to_owned(), None).suggestions[0].word,
            "receive"
        );
        assert!(
            dictionary_catalog()
                .iter()
                .any(|entry| entry.locale == "en_US")
        );

        fs::remove_dir_all(directory).expect("fixture directory is removed");
    }

    #[test]
    fn loads_a_validated_runtime_artifact_from_bytes() {
        let aff = b"SET UTF-8\n";
        let dic = b"1\nferrolex\n";
        let imported = import_bytes("fixture.aff", aff, "fixture.dic", dic, ImportMode::Strict)
            .expect("fixture imports");
        let artifact = compile_runtime_artifact(
            imported.dictionary(),
            SourceDigests::from_source_bytes(aff, dic),
        )
        .expect("artifact compiles");

        let checker = SpellChecker::from_runtime_artifact_bytes(Buffer::from(artifact), None)
            .expect("artifact loads");
        assert!(checker.check("ferrolex".to_owned()));
    }
}
