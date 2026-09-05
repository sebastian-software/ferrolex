//! Node.js binding for the focused ferrolex engine API.
//!
//! The binding mirrors the Rust concepts: immutable word-list or Hunspell
//! checkers, deterministic bounded suggestions, and caller-selected managed
//! dictionary caches.

// napi-rs generates Node-API registration glue with unsafe code. The
// handwritten adapter below remains safe Rust; core crates still forbid it.
#![allow(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use ferrolex_core::{contains_normalized, Normalization, WordList};
use ferrolex_dictionaries::{
    find_locale, DictionaryInstaller, SourceEncoding, UreqFetcher, LIBREOFFICE_CATALOG,
};
use ferrolex_hunspell::{
    import_bytes, import_bytes_with_encodings, ByteEncoding, ByteImportEncodings, ImportError,
    ImportMode, ImportResult,
};
use ferrolex_suggest::{CandidateSource, SuggestConfig, Suggester};
use napi::bindgen_prelude::AsyncTask;
use napi::{Env, Error, Result, Status, Task};
use napi_derive::napi;

enum CheckerBackend {
    WordList(WordList),
    Hunspell(Box<ImportResult>),
}

/// Immutable checker backed by a word list or strict Hunspell import.
#[napi]
pub struct Checker {
    backend: CheckerBackend,
}

#[napi]
#[allow(
    clippy::needless_pass_by_value,
    reason = "napi-rs converts JavaScript strings into owned Rust values"
)]
impl Checker {
    /// Creates a checker from UTF-8, newline-delimited word-list text.
    #[napi(constructor)]
    #[must_use]
    pub fn new(words: String) -> Self {
        Self {
            backend: CheckerBackend::WordList(WordList::from_text(Normalization::Exact, &words)),
        }
    }

    /// Creates a checker by strictly importing caller-owned Hunspell files.
    ///
    /// # Errors
    ///
    /// Returns a JavaScript error when either file cannot be read or strict
    /// import reports an unsupported or malformed recognition construct.
    #[napi(factory)]
    pub fn from_hunspell(aff_path: String, dic_path: String) -> Result<Self> {
        load_hunspell(Path::new(&aff_path), Path::new(&dic_path), None)
            .map(Self::from_hunspell_import)
            .map_err(node_error)
    }

    /// Installs and strictly imports a digest-pinned catalog dictionary.
    ///
    /// Network, verification, and import work runs outside the JavaScript
    /// event loop. The cache root is always selected by the caller.
    #[napi(ts_return_type = "Promise<Checker>")]
    #[must_use]
    pub fn install(locale: String, cache_root: String) -> AsyncTask<InstallDictionary> {
        AsyncTask::new(InstallDictionary {
            locale,
            cache_root: PathBuf::from(cache_root),
        })
    }

    /// Returns whether this checker recognizes a word.
    #[napi]
    #[must_use]
    pub fn check(&self, word: String) -> bool {
        match &self.backend {
            CheckerBackend::WordList(dictionary) => contains_normalized(dictionary, &word),
            CheckerBackend::Hunspell(import) => contains_normalized(import.dictionary(), &word),
        }
    }

    /// Returns deterministic bounded spelling suggestions for a word.
    #[napi]
    #[must_use]
    pub fn suggest(&self, word: String) -> Vec<String> {
        match &self.backend {
            CheckerBackend::WordList(dictionary) => suggestions(dictionary, &word),
            CheckerBackend::Hunspell(import) => {
                let dictionary = import.dictionary();
                Suggester::new(dictionary, SuggestConfig::default())
                    .with_replacement_rules(dictionary.replacement_rules())
                    .with_ranking_signals(dictionary.ranking_signals())
                    .suggest(&word)
                    .suggestions()
                    .iter()
                    .map(|suggestion| suggestion.word().to_owned())
                    .collect()
            }
        }
    }
}

impl Checker {
    fn from_hunspell_import(import: ImportResult) -> Self {
        Self {
            backend: CheckerBackend::Hunspell(Box::new(import)),
        }
    }
}

/// Reviewed source metadata for one managed dictionary.
#[napi(object)]
pub struct CatalogDictionary {
    /// Locale identifier accepted by `Checker.install`.
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
}

impl Task for InstallDictionary {
    type Output = Checker;
    type JsValue = Checker;

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
        .map(Checker::from_hunspell_import)
        .map_err(node_error)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

fn suggestions<S: CandidateSource + ?Sized>(dictionary: &S, word: &str) -> Vec<String> {
    Suggester::new(dictionary, SuggestConfig::default())
        .suggest(word)
        .suggestions()
        .iter()
        .map(|suggestion| suggestion.word().to_owned())
        .collect()
}

fn load_hunspell(
    aff_path: &Path,
    dic_path: &Path,
    encodings: Option<ByteImportEncodings>,
) -> std::result::Result<ImportResult, String> {
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
    imported.map_err(|error| format_import_error(&error))
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

    use super::{dictionary_catalog, Checker};

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn exposes_word_list_checking_and_suggestions() {
        let checker = Checker::new("ferrolex\nFerris".to_owned());

        assert!(checker.check("ferrolex".to_owned()));
        assert!(!checker.check("ferolex".to_owned()));
        assert_eq!(checker.suggest("ferolex".to_owned()), ["ferrolex"]);
    }

    #[test]
    fn checks_word_lists_with_nfc_fallback() {
        let checker = Checker::new("café\n".to_owned());

        assert!(checker.check("cafe\u{301}".to_owned()));
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

        let checker = Checker::from_hunspell(
            aff_path.to_string_lossy().into_owned(),
            dic_path.to_string_lossy().into_owned(),
        )
        .expect("fixture imports strictly");
        assert!(checker.check("receives".to_owned()));
        assert_eq!(checker.suggest("recieve".to_owned())[0], "receive");
        assert!(dictionary_catalog()
            .iter()
            .any(|entry| entry.locale == "en_US"));

        fs::remove_dir_all(directory).expect("fixture directory is removed");
    }
}
