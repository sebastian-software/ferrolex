//! Shared data model for Ferrolex checkers and source adapters.

/// Locale identifier associated with a source span or dictionary.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocaleCode(String);

impl LocaleCode {
    /// Creates a locale code from a caller-provided identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the locale identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Role of a text span within its source file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceRole {
    /// User-facing copy from JSON, YAML, PO, or other localization files.
    TranslationMessage,
    /// String literal or JSX text that appears to be user-facing copy.
    UiLabel,
    /// Natural-language comment.
    Comment,
    /// Code identifier such as a variable, method, type, or property name.
    Identifier,
    /// Documentation text.
    Documentation,
    /// Source text whose role is not yet known.
    Unknown,
}

/// Byte range inside a source file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceRange {
    /// Inclusive byte offset where the range starts.
    pub start: usize,
    /// Exclusive byte offset where the range ends.
    pub end: usize,
}

impl SourceRange {
    /// Creates a byte range.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// Candidate source text extracted from a file before tokenization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    /// Text content to inspect.
    pub text: String,
    /// Optional source path reported by an adapter.
    pub source_path: Option<String>,
    /// Optional byte range in the source file.
    pub range: Option<SourceRange>,
    /// Optional locale inferred from file, catalog, or caller configuration.
    pub locale: Option<LocaleCode>,
    /// Role of this span in its source context.
    pub role: SourceRole,
}

/// Token category after tokenization and identifier splitting.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TokenKind {
    /// Natural-language word.
    Word,
    /// Acronym or initialism.
    Acronym,
    /// Numeric token.
    Number,
    /// Code-like token that should usually be ignored by dictionaries.
    Code,
    /// Punctuation or separator token.
    Separator,
}

/// Token emitted by a tokenizer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    /// Original token text.
    pub text: String,
    /// Normalized token text for lookup.
    pub normalized: String,
    /// Token kind.
    pub kind: TokenKind,
    /// Optional byte range relative to the source span.
    pub range: Option<SourceRange>,
}

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    /// Informational note.
    Info,
    /// Warning that may be acceptable in some contexts.
    Warning,
    /// Error that should normally fail CI.
    Error,
}

/// Suggested replacement for a finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    /// Suggested replacement text.
    pub replacement: String,
}

/// Structured checker output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    /// Stable rule or dictionary identifier.
    pub rule_id: String,
    /// Finding severity.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Optional source range.
    pub range: Option<SourceRange>,
    /// Optional replacement suggestions.
    pub suggestions: Vec<Suggestion>,
}

/// Dictionary lookup interface used by checkers.
pub trait DictionaryProvider {
    /// Returns true when `word` is accepted by this dictionary for `locale`.
    fn accepts(&self, locale: Option<&LocaleCode>, word: &str) -> bool;

    /// Returns replacement suggestions for `word`.
    fn suggest(&self, locale: Option<&LocaleCode>, word: &str) -> Vec<Suggestion>;
}

/// Source adapter that extracts inspectable spans from a source document.
pub trait SourceAdapter {
    /// Extracts candidate text spans from `source`.
    fn extract_spans(&self, source: &str) -> Vec<Span>;
}

/// Checker that produces diagnostics from spans and tokens.
pub trait Checker {
    /// Checks a source span and its tokens.
    fn check(&self, span: &Span, tokens: &[Token]) -> Vec<Finding>;
}
