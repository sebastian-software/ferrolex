//! Generic source-code analysis for ferrolex.
//!
//! The analyzer classifies generic tokens and splits identifiers without
//! depending on a programming-language parser. Language-specific adapters can
//! provide a [`Document`] with the appropriate [`CommentSyntax`].

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::ops::Range;

use ferrolex_core::Dictionary;
use regex::Regex;

/// The classification assigned to a complete source token.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum TokenClass {
    /// A natural-language word.
    NaturalWord,
    /// A mixed-case or separator-delimited identifier.
    Identifier,
    /// An all-uppercase alphabetic token.
    Acronym,
    /// A URL-like token.
    Url,
    /// An email-address-like token.
    Email,
    /// A numeric token.
    Number,
    /// A hexadecimal hash or hexadecimal literal.
    Hash,
    /// A local or relative file-system path.
    Path,
    /// A long ASCII token shaped like padded or unpadded Base64 data.
    Base64,
    /// A token without a more specific classification.
    Unknown,
}

/// The comment syntax used to recognize ferrolex inline directives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommentSyntax {
    /// Do not recognize inline directives.
    None,
    /// Recognize directives in comments with this line prefix.
    Line(String),
    /// Recognize directives in HTML comments.
    Html,
}

impl CommentSyntax {
    /// Creates a line-comment syntax from a non-empty prefix such as `//`.
    #[must_use]
    pub fn line(prefix: impl Into<String>) -> Self {
        Self::Line(prefix.into())
    }
}

/// Source text and the context needed for generic analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document<'source> {
    source: &'source str,
    comment_syntax: CommentSyntax,
}

impl<'source> Document<'source> {
    /// Creates a document that has no recognized directive syntax.
    #[must_use]
    pub const fn new(source: &'source str) -> Self {
        Self {
            source,
            comment_syntax: CommentSyntax::None,
        }
    }

    /// Uses `comment_syntax` to recognize inline ferrolex directives.
    #[must_use]
    pub fn with_comment_syntax(mut self, comment_syntax: CommentSyntax) -> Self {
        self.comment_syntax = comment_syntax;
        self
    }

    /// Returns the original source text.
    #[must_use]
    pub fn source(&self) -> &'source str {
        self.source
    }
}

/// A source segment produced by [`split_identifier`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentifierSegment<'source> {
    text: &'source str,
    range: Range<usize>,
    is_number: bool,
}

impl<'source> IdentifierSegment<'source> {
    /// Returns the original segment text.
    #[must_use]
    pub fn text(&self) -> &'source str {
        self.text
    }

    /// Returns the UTF-8 byte range relative to the identifier input.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns whether this segment contains only Unicode decimal digits.
    #[must_use]
    pub fn is_number(&self) -> bool {
        self.is_number
    }
}

/// Controls how a leading single-letter uppercase prefix is segmented.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SingleLetterPrefix {
    /// Keep `OAuth` as a single segment.
    #[default]
    Join,
    /// Split `OAuth` into `O` and `Auth`.
    Separate,
}

/// Identifier segmentation settings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IdentifierSplitConfig {
    single_letter_prefix: SingleLetterPrefix,
}

impl IdentifierSplitConfig {
    /// Uses the supplied leading-prefix policy.
    #[must_use]
    pub const fn with_single_letter_prefix(single_letter_prefix: SingleLetterPrefix) -> Self {
        Self {
            single_letter_prefix,
        }
    }
}

/// Splits an identifier into Unicode-safe alphabetic and numeric segments.
///
/// Separators, symbols, and digit-to-letter boundaries terminate a segment.
/// Lowercase-to-uppercase and acronym-to-word boundaries create segments as
/// well. The returned text is borrowed from `identifier` without normalization.
#[must_use]
pub fn split_identifier(
    identifier: &str,
    config: IdentifierSplitConfig,
) -> Vec<IdentifierSegment<'_>> {
    let mut segments = Vec::new();
    let mut run_start = None;

    for (offset, character) in identifier.char_indices() {
        if character.is_alphanumeric() {
            run_start.get_or_insert(offset);
            continue;
        }

        if let Some(start) = run_start.take() {
            split_run(identifier, start, offset, config, &mut segments);
        }
    }

    if let Some(start) = run_start {
        split_run(identifier, start, identifier.len(), config, &mut segments);
    }

    segments
}

/// Configuration for [`Analyzer`].
#[derive(Clone, Debug)]
pub struct AnalyzerConfig {
    identifier_split: IdentifierSplitConfig,
    ignored_classes: BTreeSet<TokenClass>,
    ignored_words: BTreeSet<Box<str>>,
    ignored_patterns: Vec<Regex>,
    minimum_word_length: usize,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            identifier_split: IdentifierSplitConfig::default(),
            ignored_classes: [
                TokenClass::Url,
                TokenClass::Email,
                TokenClass::Number,
                TokenClass::Hash,
                TokenClass::Path,
                TokenClass::Base64,
            ]
            .into_iter()
            .collect(),
            ignored_words: BTreeSet::new(),
            ignored_patterns: Vec::new(),
            minimum_word_length: 2,
        }
    }
}

/// A small, deterministic project-level analysis policy.
///
/// The text form is deliberately line-oriented so it can be stored as
/// `.ferrolex/config` without introducing a generic configuration dependency:
/// `ignore-word = value`, `ignore-pattern = regex`,
/// `minimum-word-length = number`, and `single-letter-prefix = join|separate`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectConfig {
    ignored_words: BTreeSet<Box<str>>,
    ignored_patterns: BTreeSet<Box<str>>,
    minimum_word_length: Option<usize>,
    single_letter_prefix: Option<SingleLetterPrefix>,
}

impl ProjectConfig {
    /// Parses the stable line-oriented project configuration format.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectConfigError::InvalidLine`] when an entry is malformed,
    /// empty, or uses an unsupported key or value.
    pub fn from_text(text: &str) -> Result<Self, ProjectConfigError> {
        let mut config = Self::default();
        for (index, line) in text.lines().enumerate() {
            let line_number = index + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ProjectConfigError::InvalidLine {
                    line: line_number,
                    message: "expected `key = value`".to_owned(),
                });
            };
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(ProjectConfigError::InvalidLine {
                    line: line_number,
                    message: "value must not be empty".to_owned(),
                });
            }
            match key {
                "ignore-word" => {
                    config.ignored_words.insert(Box::from(value));
                }
                "ignore-pattern" => {
                    config.ignored_patterns.insert(Box::from(value));
                }
                "minimum-word-length" => {
                    let value = value.parse().map_err(|_| ProjectConfigError::InvalidLine {
                        line: line_number,
                        message: "minimum-word-length must be a non-negative integer".to_owned(),
                    })?;
                    config.minimum_word_length = Some(value);
                }
                "single-letter-prefix" => {
                    let value = match value {
                        "join" => SingleLetterPrefix::Join,
                        "separate" => SingleLetterPrefix::Separate,
                        _ => {
                            return Err(ProjectConfigError::InvalidLine {
                                line: line_number,
                                message: "single-letter-prefix must be `join` or `separate`"
                                    .to_owned(),
                            })
                        }
                    };
                    config.single_letter_prefix = Some(value);
                }
                _ => {
                    return Err(ProjectConfigError::InvalidLine {
                        line: line_number,
                        message: format!("unknown key `{key}`"),
                    })
                }
            }
        }
        Ok(config)
    }

    /// Serializes this configuration in canonical deterministic order.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for word in &self.ignored_words {
            text.push_str("ignore-word = ");
            text.push_str(word);
            text.push('\n');
        }
        for pattern in &self.ignored_patterns {
            text.push_str("ignore-pattern = ");
            text.push_str(pattern);
            text.push('\n');
        }
        if let Some(minimum) = self.minimum_word_length {
            text.push_str("minimum-word-length = ");
            text.push_str(&minimum.to_string());
            text.push('\n');
        }
        if let Some(prefix) = self.single_letter_prefix {
            text.push_str("single-letter-prefix = ");
            text.push_str(match prefix {
                SingleLetterPrefix::Join => "join",
                SingleLetterPrefix::Separate => "separate",
            });
            text.push('\n');
        }
        text
    }
}

/// A syntactic error in [`ProjectConfig`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectConfigError {
    /// A non-comment configuration line was invalid.
    InvalidLine {
        /// One-based line number.
        line: usize,
        /// A concise diagnostic.
        message: String,
    },
}

impl fmt::Display for ProjectConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLine { line, message } => {
                write!(formatter, "project config line {line}: {message}")
            }
        }
    }
}

impl Error for ProjectConfigError {}

/// A configuration error for [`AnalyzerBuilder`].
#[derive(Debug)]
pub enum AnalyzerConfigError {
    /// An ignore pattern is not a valid regular expression.
    InvalidIgnorePattern(regex::Error),
}

impl fmt::Display for AnalyzerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIgnorePattern(error) => {
                write!(formatter, "invalid ignore pattern: {error}")
            }
        }
    }
}

impl Error for AnalyzerConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIgnorePattern(error) => Some(error),
        }
    }
}

/// Configures a generic source-code [`Analyzer`].
pub struct AnalyzerBuilder<'dictionary> {
    dictionary: &'dictionary dyn Dictionary,
    config: AnalyzerConfig,
}

impl<'dictionary> AnalyzerBuilder<'dictionary> {
    /// Applies a parsed persistent project policy.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzerConfigError::InvalidIgnorePattern`] when a configured
    /// `ignore-pattern` is not a valid regular expression.
    pub fn project_config(mut self, project: &ProjectConfig) -> Result<Self, AnalyzerConfigError> {
        for word in &project.ignored_words {
            self = self.ignore_word(word.clone());
        }
        for pattern in &project.ignored_patterns {
            self = self.ignore_pattern(pattern)?;
        }
        if let Some(minimum) = project.minimum_word_length {
            self = self.minimum_word_length(minimum);
        }
        if let Some(prefix) = project.single_letter_prefix {
            self = self.identifier_split(IdentifierSplitConfig::with_single_letter_prefix(prefix));
        }
        Ok(self)
    }
    /// Ignores every token with `class`.
    #[must_use]
    pub fn ignore_class(mut self, class: TokenClass) -> Self {
        self.config.ignored_classes.insert(class);
        self
    }

    /// Removes the default or previously configured ignore for `class`.
    #[must_use]
    pub fn check_class(mut self, class: TokenClass) -> Self {
        self.config.ignored_classes.remove(&class);
        self
    }

    /// Ignores this exact source word or identifier segment.
    #[must_use]
    pub fn ignore_word(mut self, word: impl Into<Box<str>>) -> Self {
        self.config.ignored_words.insert(word.into());
        self
    }

    /// Ignores full raw tokens matching `pattern`.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzerConfigError::InvalidIgnorePattern`] when `pattern`
    /// cannot be compiled.
    pub fn ignore_pattern(mut self, pattern: &str) -> Result<Self, AnalyzerConfigError> {
        let pattern = Regex::new(pattern).map_err(AnalyzerConfigError::InvalidIgnorePattern)?;
        self.config.ignored_patterns.push(pattern);
        Ok(self)
    }

    /// Sets the shortest alphabetic segment that will be checked.
    #[must_use]
    pub fn minimum_word_length(mut self, minimum_word_length: usize) -> Self {
        self.config.minimum_word_length = minimum_word_length;
        self
    }

    /// Sets the identifier-segmentation policy.
    #[must_use]
    pub fn identifier_split(mut self, identifier_split: IdentifierSplitConfig) -> Self {
        self.config.identifier_split = identifier_split;
        self
    }

    /// Builds the analyzer.
    #[must_use]
    pub fn build(self) -> Analyzer<'dictionary> {
        Analyzer {
            dictionary: self.dictionary,
            config: self.config,
        }
    }
}

/// A generic source-code spell checker.
pub struct Analyzer<'dictionary> {
    dictionary: &'dictionary dyn Dictionary,
    config: AnalyzerConfig,
}

impl<'dictionary> Analyzer<'dictionary> {
    /// Starts configuring analysis against `dictionary`.
    #[must_use]
    pub fn builder(dictionary: &'dictionary impl Dictionary) -> AnalyzerBuilder<'dictionary> {
        AnalyzerBuilder {
            dictionary,
            config: AnalyzerConfig::default(),
        }
    }

    /// Checks a document and returns findings plus directive diagnostics.
    #[must_use]
    pub fn check<'source>(&self, document: &Document<'source>) -> Analysis<'source> {
        let lines = document_lines(document.source);
        let mut directives = DirectiveState::default();
        let mut directive_lines = BTreeSet::new();
        let mut diagnostics = Vec::new();

        for (line_index, line) in lines.iter().enumerate() {
            if let Some(directive) = parse_directive(line.text, &document.comment_syntax) {
                directive_lines.insert(line_index);
                directives.record_ignore_words(&directive);
                if let Some(problem) = directive.problem() {
                    diagnostics.push(DirectiveDiagnostic {
                        range: line.start..line.end,
                        problem,
                    });
                }
            }
        }

        let mut findings = Vec::new();
        for (line_index, line) in lines.iter().enumerate() {
            if let Some(directive) = parse_directive(line.text, &document.comment_syntax) {
                directives.apply_switch(&directive);
                continue;
            }
            if directive_lines.contains(&line_index) || directives.disabled {
                continue;
            }

            for raw_token in raw_tokens(line.text, line.start) {
                self.check_token(
                    document.source,
                    &raw_token,
                    &directives.ignored_words,
                    &mut findings,
                );
            }
        }

        Analysis {
            findings,
            directive_diagnostics: diagnostics,
        }
    }

    fn check_token<'source>(
        &self,
        source: &'source str,
        raw_token: &RawToken<'source>,
        directive_ignored_words: &BTreeSet<Box<str>>,
        findings: &mut Vec<Finding<'source>>,
    ) {
        let class = classify(raw_token.text);
        if self.config.ignored_classes.contains(&class)
            || self.matches_ignored_pattern(raw_token.text)
            || self.config.ignored_words.contains(raw_token.text)
            || directive_ignored_words.contains(raw_token.text)
        {
            return;
        }

        let segments = split_identifier(raw_token.text, self.config.identifier_split);
        let is_identifier = class == TokenClass::Identifier;

        for (segment_index, segment) in segments.iter().enumerate() {
            if segment.is_number()
                || segment.text().chars().count() < self.config.minimum_word_length
            {
                continue;
            }
            if self.config.ignored_words.contains(segment.text())
                || directive_ignored_words.contains(segment.text())
                || self.dictionary.contains(segment.text())
            {
                continue;
            }

            let range = shift_range(segment.range(), raw_token.range.start);
            findings.push(Finding {
                word: &source[range.clone()],
                range,
                token: raw_token.text,
                token_range: raw_token.range.clone(),
                class,
                segment_index: is_identifier.then_some(segment_index),
            });
        }
    }

    fn matches_ignored_pattern(&self, token: &str) -> bool {
        self.config.ignored_patterns.iter().any(|pattern| {
            pattern
                .find(token)
                .is_some_and(|matched| matched.start() == 0 && matched.end() == token.len())
        })
    }
}

/// A spelling finding for one word or identifier segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding<'source> {
    word: &'source str,
    range: Range<usize>,
    token: &'source str,
    token_range: Range<usize>,
    class: TokenClass,
    segment_index: Option<usize>,
}

impl<'source> Finding<'source> {
    /// Returns the original misspelled word or identifier segment.
    #[must_use]
    pub fn word(&self) -> &'source str {
        self.word
    }

    /// Returns the UTF-8 byte range of the word or segment.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns the complete raw token that produced this finding.
    #[must_use]
    pub fn token(&self) -> &'source str {
        self.token
    }

    /// Returns the UTF-8 byte range of the complete raw token.
    #[must_use]
    pub fn token_range(&self) -> Range<usize> {
        self.token_range.clone()
    }

    /// Returns the classification of the complete raw token.
    #[must_use]
    pub fn class(&self) -> TokenClass {
        self.class
    }

    /// Returns this segment's position within an identifier, when applicable.
    #[must_use]
    pub fn segment_index(&self) -> Option<usize> {
        self.segment_index
    }
}

/// Replaces an identifier finding with `suggestion` in its complete token.
///
/// The replacement follows the finding's lower-, upper-, or initial-uppercase
/// casing so callers can offer one whole-identifier edit.
#[must_use]
pub fn recombine_identifier_suggestion(finding: &Finding<'_>, suggestion: &str) -> Option<String> {
    finding.segment_index?;
    let relative_start = finding.range.start.checked_sub(finding.token_range.start)?;
    let relative_end = finding.range.end.checked_sub(finding.token_range.start)?;
    let replacement = preserve_segment_casing(finding.word, suggestion);
    let mut token = String::with_capacity(finding.token.len() + replacement.len());
    token.push_str(&finding.token[..relative_start]);
    token.push_str(&replacement);
    token.push_str(&finding.token[relative_end..]);
    Some(token)
}

fn preserve_segment_casing(original: &str, suggestion: &str) -> String {
    if original.chars().all(char::is_lowercase) {
        return suggestion.to_lowercase();
    }
    if original.chars().all(char::is_uppercase) {
        return suggestion.to_uppercase();
    }
    let mut original_characters = original.chars();
    if original_characters.next().is_some_and(char::is_uppercase)
        && original_characters.all(char::is_lowercase)
    {
        let mut characters = suggestion.chars();
        let Some(first) = characters.next() else {
            return String::new();
        };
        return first
            .to_uppercase()
            .chain(characters.flat_map(char::to_lowercase))
            .collect();
    }
    suggestion.to_owned()
}

/// A non-fatal inline-directive error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectiveDiagnostic {
    range: Range<usize>,
    problem: DirectiveProblem,
}

impl DirectiveDiagnostic {
    /// Returns the source range of the malformed directive line.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns the structured reason for the diagnostic.
    #[must_use]
    pub fn problem(&self) -> DirectiveProblem {
        self.problem
    }
}

/// The reason an inline directive is malformed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DirectiveProblem {
    /// `ferrolex:ignore` did not name a word.
    MissingIgnoredWords,
    /// `ferrolex:disable` or `ferrolex:enable` received arguments.
    UnexpectedArguments,
    /// The directive name is not recognized.
    UnknownDirective,
}

/// The result of a complete document analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Analysis<'source> {
    findings: Vec<Finding<'source>>,
    directive_diagnostics: Vec<DirectiveDiagnostic>,
}

impl<'source> Analysis<'source> {
    /// Returns all misspelled source segments in deterministic source order.
    #[must_use]
    pub fn findings(&self) -> &[Finding<'source>] {
        &self.findings
    }

    /// Returns non-fatal malformed-directive diagnostics.
    #[must_use]
    pub fn directive_diagnostics(&self) -> &[DirectiveDiagnostic] {
        &self.directive_diagnostics
    }
}

#[derive(Clone, Copy)]
struct Line<'source> {
    text: &'source str,
    start: usize,
    end: usize,
}

fn document_lines(source: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;

    for line in source.split_inclusive('\n') {
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let text = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);
        let end = start + text.len();
        lines.push(Line { text, start, end });
        start += line.len();
    }

    if source.is_empty() || (!source.ends_with('\n') && start < source.len()) {
        let text = &source[start..];
        lines.push(Line {
            text,
            start,
            end: source.len(),
        });
    }

    lines
}

#[derive(Default)]
struct DirectiveState {
    disabled: bool,
    ignored_words: BTreeSet<Box<str>>,
}

impl DirectiveState {
    fn record_ignore_words(&mut self, directive: &Directive<'_>) {
        if let DirectiveKind::Ignore(words) = &directive.kind {
            self.ignored_words
                .extend(words.iter().copied().map(Box::<str>::from));
        }
    }

    fn apply_switch(&mut self, directive: &Directive<'_>) {
        match directive.kind {
            DirectiveKind::Disable => self.disabled = true,
            DirectiveKind::Enable => self.disabled = false,
            DirectiveKind::Ignore(_) | DirectiveKind::Problem(_) => {}
        }
    }
}

enum DirectiveKind<'source> {
    Ignore(Vec<&'source str>),
    Disable,
    Enable,
    Problem(DirectiveProblem),
}

struct Directive<'source> {
    kind: DirectiveKind<'source>,
}

impl Directive<'_> {
    fn problem(&self) -> Option<DirectiveProblem> {
        match self.kind {
            DirectiveKind::Problem(problem) => Some(problem),
            DirectiveKind::Ignore(_) | DirectiveKind::Disable | DirectiveKind::Enable => None,
        }
    }
}

fn parse_directive<'source>(
    line: &'source str,
    syntax: &CommentSyntax,
) -> Option<Directive<'source>> {
    let comment = match syntax {
        CommentSyntax::None => return None,
        CommentSyntax::Line(prefix) => line.trim_start().strip_prefix(prefix)?.trim_start(),
        CommentSyntax::Html => line
            .trim_start()
            .strip_prefix("<!--")?
            .trim_start()
            .strip_suffix("-->")?
            .trim_end(),
    };
    let content = comment.strip_prefix("ferrolex:")?;
    let mut parts = content.split_whitespace();
    let name = parts.next()?;
    let arguments = parts.collect::<Vec<_>>();

    let kind = match name {
        "ignore" if arguments.is_empty() => {
            DirectiveKind::Problem(DirectiveProblem::MissingIgnoredWords)
        }
        "ignore" => DirectiveKind::Ignore(arguments),
        "disable" if arguments.is_empty() => DirectiveKind::Disable,
        "enable" if arguments.is_empty() => DirectiveKind::Enable,
        "disable" | "enable" => DirectiveKind::Problem(DirectiveProblem::UnexpectedArguments),
        _ => DirectiveKind::Problem(DirectiveProblem::UnknownDirective),
    };

    Some(Directive { kind })
}

#[derive(Clone)]
struct RawToken<'source> {
    text: &'source str,
    range: Range<usize>,
}

fn raw_tokens(line: &str, line_start: usize) -> Vec<RawToken<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;

    for (offset, character) in line.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = start.take() {
                push_raw_token(line, line_start, start, offset, &mut tokens);
            }
        } else {
            start.get_or_insert(offset);
        }
    }
    if let Some(start) = start {
        push_raw_token(line, line_start, start, line.len(), &mut tokens);
    }

    tokens
}

fn push_raw_token<'source>(
    line: &'source str,
    line_start: usize,
    start: usize,
    end: usize,
    tokens: &mut Vec<RawToken<'source>>,
) {
    let token = &line[start..end];
    let trimmed = token.trim_matches(|character: char| {
        character.is_ascii_punctuation() && !matches!(character, '_' | '-' | '.' | '@' | ':' | '/')
    });
    if trimmed.is_empty() {
        return;
    }
    let leading = token
        .find(trimmed)
        .expect("trimmed token is a substring of its source token");
    let start = line_start + start + leading;
    let end = start + trimmed.len();
    tokens.push(RawToken {
        text: trimmed,
        range: start..end,
    });
}

fn classify(token: &str) -> TokenClass {
    if token.contains("://") || token.starts_with("www.") {
        TokenClass::Url
    } else if token.matches('@').count() == 1
        && token
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
    {
        TokenClass::Email
    } else if is_hex_token(token) {
        TokenClass::Hash
    } else if is_path_token(token) {
        TokenClass::Path
    } else if is_base64_token(token) {
        TokenClass::Base64
    } else if token.chars().all(char::is_numeric) {
        TokenClass::Number
    } else if split_identifier(token, IdentifierSplitConfig::default()).len() > 1 {
        TokenClass::Identifier
    } else if token.chars().all(char::is_alphabetic) && token.chars().all(char::is_uppercase) {
        TokenClass::Acronym
    } else if token.chars().all(char::is_alphabetic) {
        TokenClass::NaturalWord
    } else {
        TokenClass::Unknown
    }
}

fn is_path_token(token: &str) -> bool {
    let normalized = token.replace('\\', "/");
    let mut parts = normalized.split('/');
    let first = parts.next().unwrap_or_default();
    let has_separator = normalized.contains('/');
    has_separator
        && !first.is_empty()
        && parts.any(|part| !part.is_empty() && part != "." && part != "..")
}

fn is_base64_token(token: &str) -> bool {
    token.len() >= 24
        && token.is_ascii()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        && (token.contains(['+', '/', '=']) || token.bytes().any(|byte| byte.is_ascii_digit()))
}

fn is_hex_token(token: &str) -> bool {
    let hexadecimal = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
        .unwrap_or(token);
    hexadecimal.len() >= 7
        && hexadecimal
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn split_run<'source>(
    identifier: &'source str,
    start: usize,
    end: usize,
    config: IdentifierSplitConfig,
    segments: &mut Vec<IdentifierSegment<'source>>,
) {
    let characters = identifier[start..end].char_indices().collect::<Vec<_>>();
    let mut boundaries = vec![0];

    for index in 1..characters.len() {
        let previous = characters[index - 1].1;
        let current = characters[index].1;
        let next = characters.get(index + 1).map(|(_, character)| *character);
        let changes_kind = previous.is_numeric() != current.is_numeric();
        let starts_word = current.is_uppercase()
            && (previous.is_lowercase()
                || (previous.is_uppercase() && next.is_some_and(char::is_lowercase)));
        if changes_kind || starts_word {
            boundaries.push(characters[index].0);
        }
    }
    boundaries.push(end - start);

    let mut run_segments = boundaries
        .windows(2)
        .map(|boundary| {
            let range = (start + boundary[0])..(start + boundary[1]);
            IdentifierSegment {
                text: &identifier[range.clone()],
                is_number: identifier[range.clone()].chars().all(char::is_numeric),
                range,
            }
        })
        .collect::<Vec<_>>();

    if config.single_letter_prefix == SingleLetterPrefix::Join && run_segments.len() >= 2 {
        let first_range = run_segments[0].range.clone();
        let joins_prefix = run_segments[0].text.chars().count() == 1
            && run_segments[0].text.chars().all(char::is_uppercase)
            && !run_segments[1].is_number;
        if joins_prefix {
            let second = run_segments.remove(1);
            run_segments[0] = IdentifierSegment {
                text: &identifier[first_range.start..second.range.end],
                range: first_range.start..second.range.end,
                is_number: false,
            };
        }
    }

    segments.extend(run_segments);
}

fn shift_range(range: Range<usize>, offset: usize) -> Range<usize> {
    (range.start + offset)..(range.end + offset)
}

#[cfg(test)]
mod tests {
    use ferrolex_core::{Checker, Normalization, UserDictionary, WordList};

    use super::{
        classify, recombine_identifier_suggestion, split_identifier, Analyzer, CommentSyntax,
        DirectiveProblem, Document, IdentifierSplitConfig, ProjectConfig, ProjectConfigError,
        SingleLetterPrefix, TokenClass,
    };

    #[test]
    fn splits_rfc_identifiers_with_unicode_and_digits() {
        let segments = |identifier| {
            split_identifier(identifier, IdentifierSplitConfig::default())
                .into_iter()
                .map(|segment| segment.text().to_owned())
                .collect::<Vec<_>>()
        };

        assert_eq!(segments("userAuthenticator"), ["user", "Authenticator"]);
        assert_eq!(
            segments("OAuthAuthenticationProvider"),
            ["OAuth", "Authentication", "Provider"]
        );
        assert_eq!(segments("HTTPResponseCode"), ["HTTP", "Response", "Code"]);
        assert_eq!(segments("user_profile_image"), ["user", "profile", "image"]);
        assert_eq!(segments("StraßeÜberblick"), ["Straße", "Überblick"]);
        assert_eq!(segments("version2Parser"), ["version", "2", "Parser"]);
    }

    #[test]
    fn supports_the_alternative_single_letter_prefix_policy() {
        let segments = split_identifier(
            "OAuth",
            IdentifierSplitConfig::with_single_letter_prefix(SingleLetterPrefix::Separate),
        );

        assert_eq!(
            segments
                .into_iter()
                .map(|segment| segment.text())
                .collect::<Vec<_>>(),
            ["O", "Auth"]
        );
    }

    #[test]
    fn project_configuration_round_trips_and_changes_analysis() {
        let config = ProjectConfig::from_text(
            "# project policy\nignore-word = Ferrolex\nignore-pattern = ^generated_[a-z]+$\nminimum-word-length = 3\nsingle-letter-prefix = separate\n",
        )
        .expect("configuration is valid");
        assert_eq!(
            ProjectConfig::from_text(&config.to_text()),
            Ok(config.clone())
        );

        let dictionary = WordList::new(["Auth"]).expect("test words are valid");
        let analyzer = Analyzer::builder(&dictionary)
            .project_config(&config)
            .expect("pattern compiles")
            .build();
        let analysis = analyzer.check(&Document::new("Ferrolex generated_token OAuth"));

        assert!(analysis.findings().is_empty());
    }

    #[test]
    fn project_configuration_reports_line_or_pattern_errors() {
        assert_eq!(
            ProjectConfig::from_text("unknown = value\n"),
            Err(ProjectConfigError::InvalidLine {
                line: 1,
                message: "unknown key `unknown`".to_owned(),
            })
        );

        let config =
            ProjectConfig::from_text("ignore-pattern = [\n").expect("syntax alone is preserved");
        let empty = WordList::new(std::iter::empty::<&str>()).expect("empty dictionary is valid");
        assert!(Analyzer::builder(&empty).project_config(&config).is_err());
    }

    #[test]
    fn classifies_unambiguous_special_tokens_before_identifiers() {
        assert_eq!(classify("https://example.com"), TokenClass::Url);
        assert_eq!(classify("maintainer@example.com"), TokenClass::Email);
        assert_eq!(classify("0xdeadbeef"), TokenClass::Hash);
        assert_eq!(classify("2026"), TokenClass::Number);
        assert_eq!(
            classify("crates/ferrolex-code/src/lib.rs"),
            TokenClass::Path
        );
        assert_eq!(
            classify("QXV0aGVudGljYXRpb25Qcm92aWRlcg=="),
            TokenClass::Base64
        );
        assert_eq!(classify("HTTP"), TokenClass::Acronym);
    }

    #[test]
    fn checks_identifier_segments_and_preserves_the_whole_identifier() {
        let dictionary = WordList::new(["let", "user", "Authentication", "Provider", "build"])
            .expect("test words are valid");
        let analyzer = Analyzer::builder(&dictionary).build();
        let source = "let userAuthentcationProvider = build();";

        let analysis = analyzer.check(&Document::new(source));
        let finding = analysis.findings().first().expect("the typo is reported");

        assert_eq!(finding.word(), "Authentcation");
        assert_eq!(finding.token(), "userAuthentcationProvider");
        assert_eq!(finding.class(), TokenClass::Identifier);
        assert_eq!(&source[finding.range()], finding.word());
        assert_eq!(&source[finding.token_range()], finding.token());
    }

    #[test]
    fn ignores_default_machine_tokens_and_full_regex_matches() {
        let dictionary = WordList::new(["generated", "token"]).expect("test words are valid");
        let analyzer = Analyzer::builder(&dictionary)
            .ignore_pattern("generated_[A-Za-z]+")
            .expect("pattern is valid")
            .build();
        let source = "https://example.com maintainer@example.com 0xdeadbeef crates/ferrolex-code/src/lib.rs QXV0aGVudGljYXRpb25Qcm92aWRlcg== generated_value";

        assert!(analyzer.check(&Document::new(source)).findings().is_empty());
    }

    #[test]
    fn directives_only_apply_inside_configured_comments() {
        let dictionary = WordList::new(["known"]).expect("test words are valid");
        let analyzer = Analyzer::builder(&dictionary).build();
        let source = "// ferrolex:ignore local\nlocal typo\n// ferrolex:disable\ntypo\n// ferrolex:enable\ntypo";

        let analysis =
            analyzer.check(&Document::new(source).with_comment_syntax(CommentSyntax::line("//")));

        assert_eq!(
            analysis
                .findings()
                .iter()
                .map(super::Finding::word)
                .collect::<Vec<_>>(),
            ["typo", "typo"]
        );
        assert!(analyzer
            .check(&Document::new(source))
            .findings()
            .iter()
            .any(|finding| finding.word() == "local"));
    }

    #[test]
    fn reports_malformed_directives_without_stopping_analysis() {
        let dictionary = WordList::new(["known"]).expect("test words are valid");
        let analyzer = Analyzer::builder(&dictionary).build();
        let source = "# ferrolex:ignore\nunknown";

        let analysis =
            analyzer.check(&Document::new(source).with_comment_syntax(CommentSyntax::line("#")));

        assert_eq!(
            analysis.directive_diagnostics()[0].problem(),
            DirectiveProblem::MissingIgnoredWords
        );
        assert_eq!(analysis.findings()[0].word(), "unknown");
    }

    #[test]
    fn project_overlay_participates_in_code_analysis_immediately() {
        let base = WordList::new(["Project"]).expect("test words are valid");
        let overlay = UserDictionary::new(Normalization::Exact);
        overlay.insert("Ferrolex").expect("word is valid");
        let checker = Checker::builder()
            .dictionary(base)
            .dictionary(overlay)
            .build();
        let analyzer = Analyzer::builder(&checker).build();

        assert!(analyzer
            .check(&Document::new("FerrolexProject"))
            .findings()
            .is_empty());
    }

    #[test]
    fn recombines_identifier_suggestions_with_segment_casing() {
        let dictionary = WordList::new(["OAuth", "Provider"]).expect("test words");
        let analyzer = Analyzer::builder(&dictionary).build();
        let analysis = analyzer.check(&Document::new("OAuthAuthentcationProvider"));
        let finding = analysis.findings().first().expect("one misspelled segment");

        assert_eq!(
            recombine_identifier_suggestion(finding, "authentication"),
            Some("OAuthAuthenticationProvider".to_owned())
        );
    }
}
