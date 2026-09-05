//! Hunspell-compatible dictionary import for ferrolex.
//!
//! The importer accepts a deliberately documented subset of the textual
//! Hunspell format and translates it into ferrolex-owned data structures. No
//! runtime dependency on another spell checker is introduced.
//!
//! ```
//! use ferrolex_core::Dictionary;
//! use ferrolex_hunspell::{import, ImportMode};
//!
//! let result = import("example.aff", "SET UTF-8\n", "example.dic", "1\nferrolex\n", ImportMode::Strict)?;
//! assert!(result.dictionary().contains("ferrolex"));
//! # Ok::<(), ferrolex_hunspell::ImportError>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cache;
mod explanation;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, OnceLock};

use encoding_rs::ISO_8859_2;
use ferrolex_compiler::{
    AffixKindIr, AffixRuleIr, BreakPatternIr, CaseLanguageIr, CompoundConfigIr, CompoundPatternIr,
    CompoundSyllableLimitIr, ConditionAtomIr, ConditionIr, DictionaryIr, FlagIr, FlagModeIr,
    InputConversionIr, LexemeIr, ReplacementRuleIr, SpecialFlagsIr,
};
use ferrolex_core::{CandidateIndex, Dictionary};
use ferrolex_suggest::{CandidateSource, RankingSignals, ReplacementRule};

pub use cache::{
    compile_runtime_artifact, compile_runtime_cache, inspect_runtime_cache, is_runtime_artifact,
    load_runtime_artifact, load_runtime_cache, CacheSource, RuntimeCacheError,
    RuntimeCacheMetadata, SourceDigests, HUNSPELL_CACHE_FORMAT_VERSION,
    HUNSPELL_CACHE_SEMANTICS_VERSION,
};
pub use explanation::{
    Acceptance, AcceptanceKind, AppliedAffix, AppliedAffixKind, CasingPath, CompoundComponent,
    CompoundComponentRole, LookupExplanation, Rejection, RejectionReason,
};

const MAX_AFF_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIC_BYTES: usize = 64 * 1024 * 1024;
// The digest-pinned tr_TR fixture needs at most 22,835 bytes on one entry line.
const MAX_LINE_BYTES: usize = 32 * 1024;
const MAX_AFFIX_RULES: usize = 100_000;
const MAX_DICTIONARY_ENTRIES: usize = 1_000_000;
// The digest-pinned tr_TR fixture needs at most 3,926 numeric flags on one entry.
const MAX_FLAGS_PER_ENTRY: usize = 4_096;
const MAX_CONDITION_ATOMS: usize = 256;
const MAX_AFFIX_CHAIN: usize = 8;
const MAX_DERIVATIONS_PER_LEXEME: usize = 4_096;
/// Bounds reverse-affix candidate work for one lookup. The limit is deliberately
/// lower than the import entry limit so a suffix with an empty `add` cannot turn
/// a miss into a scan of the whole dictionary.
const MAX_DERIVED_CANDIDATES_PER_LOOKUP: usize = 8_192;
/// Bounds reverse forms used to resolve empty-add affix chains without scanning
/// every lexeme carrying the rule's origin flag.
const MAX_REVERSE_FORMS_PER_LOOKUP: usize = 4_096;
/// Caps local suggestion expansion from one query-aligned stem.
const MAX_SUGGESTION_FORMS_PER_SEED: usize = 64;
/// Caps query split positions considered for one compound suggestion seed.
const MAX_SUGGESTION_COMPOUND_SPLITS: usize = 64;
const MAX_COMPOUND_SCALARS: usize = 256;
const MAX_COMPOUND_RULES: usize = 1_024;
const MAX_COMPOUND_PATTERNS: usize = 1_024;
const MAX_COMPOUND_PATTERN_REPLACEMENT_VARIANTS: usize = 32;
const MAX_COMPOUND_RULE_COMPONENTS: usize = 16;
const MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE: usize = 1_024;
const MAX_COMPOUND_RULE_EXPANSIONS: usize = 16_384;
const MAX_BREAK_PATTERNS: usize = 256;
const MAX_REPLACEMENT_RULES: usize = 4_096;
const MAX_CHARACTER_MAPS: usize = 4_096;
const MAX_AFFIX_ALIASES: usize = 100_000;
const MAX_INPUT_CONVERSIONS: usize = 4_096;
const MAX_MORPHOLOGY_STRINGS: usize = 1_000_000;
const MAX_MORPHOLOGY_FIELDS_PER_RECORD: usize = 256;

/// Selects whether importer diagnostics prevent a dictionary from loading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImportMode {
    /// Return supported content and all diagnostics.
    #[default]
    Lenient,
    /// Reject an import that has an error diagnostic.
    Strict,
}

/// A byte encoding accepted by the byte-oriented Hunspell importer.
///
/// [`import_bytes`] discovers this encoding from the `SET` declaration in the
/// affix file. [`import_bytes_with_encodings`] accepts an explicit pair for a
/// reviewed source whose files use different encodings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteEncoding {
    /// UTF-8, decoded without replacement.
    Utf8,
    /// ISO-8859-1, decoded with its one-code-point-per-byte mapping.
    Iso8859_1,
    /// ISO-8859-2, decoded with the standard ISO-8859-2 mapping.
    Iso8859_2,
    /// UTF-8 with a per-byte ISO-8859-2 fallback for malformed affix-source
    /// bytes.
    ///
    /// This is for reviewed legacy sources that declare UTF-8 but contain a
    /// small number of ISO-8859-2 bytes. It is never selected from `SET`.
    Utf8WithIso8859_2Fallback,
}

impl ByteEncoding {
    fn from_set_label(label: &str) -> Option<Self> {
        match label.to_ascii_uppercase().as_str() {
            "UTF-8" | "UTF8" => Some(Self::Utf8),
            "ISO-8859-1" | "ISO8859-1" => Some(Self::Iso8859_1),
            "ISO-8859-2" | "ISO8859-2" => Some(Self::Iso8859_2),
            _ => None,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Iso8859_1 => "ISO-8859-1",
            Self::Iso8859_2 => "ISO-8859-2",
            Self::Utf8WithIso8859_2Fallback => "UTF-8 with ISO-8859-2 fallback",
        }
    }
}

/// Independent byte encodings for an affix file and its word list.
///
/// Most Hunspell pairs use one encoding declared by the affix file, so callers
/// should prefer [`import_bytes`]. This type exists for reviewed exceptional
/// pairs where the word list's encoding is known independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteImportEncodings {
    aff: ByteEncoding,
    dic: ByteEncoding,
}

impl ByteImportEncodings {
    /// Creates a byte encoding pair for one Hunspell affix and dictionary file.
    #[must_use]
    pub const fn new(aff: ByteEncoding, dic: ByteEncoding) -> Self {
        Self { aff, dic }
    }

    /// Creates a pair where both files use the same encoding.
    #[must_use]
    pub const fn same(encoding: ByteEncoding) -> Self {
        Self::new(encoding, encoding)
    }

    /// Returns the configured affix-file encoding.
    #[must_use]
    pub const fn aff(self) -> ByteEncoding {
        self.aff
    }

    /// Returns the configured dictionary-file encoding.
    #[must_use]
    pub const fn dic(self) -> ByteEncoding {
        self.dic
    }
}

/// The severity assigned to an import diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    /// The input cannot be interpreted safely with the supported subset.
    Error,
    /// A recognized but unsupported feature was omitted predictably.
    Warning,
}

/// A location-aware importer diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    source: String,
    line: usize,
    directive: String,
    severity: Severity,
    message: String,
}

impl Diagnostic {
    /// Returns the source name provided to the importer.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the one-based source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// Returns the directive or input component that caused the diagnostic.
    #[must_use]
    pub fn directive(&self) -> &str {
        &self.directive
    }

    /// Returns the diagnostic severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns a human-readable explanation.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// An import rejected in [`ImportMode::Strict`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportError {
    diagnostics: Vec<Diagnostic>,
}

impl ImportError {
    /// Returns all diagnostics produced before strict import failed.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Hunspell import failed with {} diagnostic(s)",
            self.diagnostics.len()
        )
    }
}

impl std::error::Error for ImportError {}

/// A parsed dictionary plus non-fatal diagnostics.
#[derive(Clone, Debug)]
pub struct ImportResult {
    dictionary: HunspellDictionary,
    ir: DictionaryIr,
    diagnostics: Vec<Diagnostic>,
}

impl ImportResult {
    /// Returns the independently represented runtime dictionary.
    #[must_use]
    pub fn dictionary(&self) -> &HunspellDictionary {
        &self.dictionary
    }

    /// Returns the source-neutral semantic representation used for compilation.
    #[must_use]
    pub fn ir(&self) -> &DictionaryIr {
        &self.ir
    }

    /// Returns warnings and lenient-mode errors encountered during import.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// An immutable dictionary imported from an `.aff`/`.dic` pair.
///
/// Stems stay in a sorted, read-only set. Affixes are evaluated lazily on
/// lookup, so importing does not pre-expand a potentially unbounded word set.
#[derive(Clone, Debug, Default)]
pub struct HunspellDictionary {
    flag_mode: FlagMode,
    case_fallback: bool,
    case_language: CaseLanguage,
    unique_stem_indices: Vec<u32>,
    morphology: MorphologyTable,
    lexemes: Vec<Lexeme>,
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    prefix_rules_by_add_edge: AffixRuleIndex,
    suffix_rules_by_add_edge: AffixRuleIndex,
    prefix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    suffix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    lexeme_indices_by_flag: BTreeMap<Flag, Vec<usize>>,
    prefix_parent_flags: BTreeMap<Flag, BTreeSet<Flag>>,
    suffix_parent_flags: BTreeMap<Flag, BTreeSet<Flag>>,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
    break_patterns: Vec<BreakPattern>,
    sharp_uppercase_forms: BTreeSet<Box<str>>,
    word_characters: BTreeSet<char>,
    replacement_rules: Vec<ReplacementRule>,
    keyboard: Option<Box<str>>,
    character_maps: Vec<String>,
    ignored_characters: BTreeSet<char>,
    input_conversions: Vec<InputConversion>,
    output_conversions: Vec<InputConversion>,
    full_strip: bool,
    complex_prefixes: bool,
    candidate_index: Arc<OnceLock<CandidateIndex>>,
}

impl Dictionary for HunspellDictionary {
    fn contains(&self, word: &str) -> bool {
        let word = self.normalize_input(word);
        self.contains_normalized(word.as_ref(), true)
            || self
                .case_folded_candidates(word.as_ref())
                .into_iter()
                .any(|candidate| self.contains_normalized(&candidate, false))
    }
}

impl HunspellDictionary {
    /// Lowers the immutable runtime dictionary into source-neutral semantics.
    ///
    /// Derived indexes and caches are intentionally omitted. The returned IR
    /// owns every declared field required to rebuild recognition behavior.
    #[must_use]
    pub fn to_ir(&self) -> DictionaryIr {
        DictionaryIr {
            flag_mode: flag_mode_to_ir(self.flag_mode),
            case_fallback: self.case_fallback,
            case_language: case_language_to_ir(self.case_language),
            morphology: self
                .morphology
                .values_by_id()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            lexemes: self
                .lexemes
                .iter()
                .map(|lexeme| lexeme_to_ir(lexeme, self.flag_mode))
                .collect(),
            prefixes: self
                .prefixes
                .iter()
                .map(|rule| affix_rule_to_ir(rule, self.flag_mode))
                .collect(),
            suffixes: self
                .suffixes
                .iter()
                .map(|rule| affix_rule_to_ir(rule, self.flag_mode))
                .collect(),
            special_flags: special_flags_to_ir(&self.special_flags, self.flag_mode),
            compound: compound_to_ir(&self.compound, self.flag_mode),
            break_patterns: self
                .break_patterns
                .iter()
                .map(break_pattern_to_ir)
                .collect(),
            word_characters: self.word_characters.clone(),
            replacement_rules: self
                .replacement_rules
                .iter()
                .map(replacement_rule_to_ir)
                .collect(),
            keyboard: self.keyboard.as_deref().map(str::to_owned),
            character_maps: self
                .character_maps
                .iter()
                .map(ToString::to_string)
                .collect(),
            ignored_characters: self.ignored_characters.clone(),
            input_conversions: self
                .input_conversions
                .iter()
                .map(input_conversion_to_ir)
                .collect(),
            output_conversions: self
                .output_conversions
                .iter()
                .map(input_conversion_to_ir)
                .collect(),
            full_strip: self.full_strip,
            complex_prefixes: self.complex_prefixes,
        }
    }

    fn contains_normalized(&self, word: &str, allow_keep_case: bool) -> bool {
        self.matches_without_break(word, allow_keep_case)
            || self.matches_break_word(word, allow_keep_case)
    }

    fn matches_without_break(&self, word: &str, allow_keep_case: bool) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && !self.requires_affix(&lexeme.flags)
                && !self.is_only_in_compound(&lexeme.flags)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        }) || self.matches_single_affix_word(word, allow_keep_case)
            || self
                .derived_candidate_indices(word)
                .into_iter()
                .flatten()
                .any(|index| self.matches_derived_word(&self.lexemes[index], word, allow_keep_case))
            || self.matches_simple_compound(word, allow_keep_case)
            || (allow_keep_case && self.sharp_uppercase_forms.contains(word))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the importer and cache hand over every owned runtime section explicitly"
    )]
    fn from_parts(
        flag_mode: FlagMode,
        case_fallback: bool,
        case_language: CaseLanguage,
        morphology: MorphologyTable,
        lexemes: Vec<Lexeme>,
        prefixes: Vec<AffixRule>,
        suffixes: Vec<AffixRule>,
        special_flags: SpecialFlags,
        compound: CompoundConfig,
        break_patterns: Vec<BreakPattern>,
        word_characters: BTreeSet<char>,
        replacement_rules: Vec<ReplacementRule>,
        keyboard: Option<Box<str>>,
        character_maps: Vec<String>,
        ignored_characters: BTreeSet<char>,
        input_conversions: Vec<InputConversion>,
        output_conversions: Vec<InputConversion>,
        full_strip: bool,
        complex_prefixes: bool,
    ) -> Self {
        let unique_stem_indices = unique_stem_indices(&lexemes);
        let prefix_rules_by_flag = rule_indices_by_flag(&prefixes);
        let suffix_rules_by_flag = rule_indices_by_flag(&suffixes);
        let prefix_rules_by_add_edge = AffixRuleIndex::new(&prefixes, AffixKind::Prefix);
        let suffix_rules_by_add_edge = AffixRuleIndex::new(&suffixes, AffixKind::Suffix);
        let lexeme_indices_by_flag = lexeme_indices_by_flag(&lexemes);
        let prefix_parent_flags = parent_flags_by_continuation(&prefixes);
        let suffix_parent_flags = parent_flags_by_continuation(&suffixes);
        let sharp_uppercase_forms = sharp_uppercase_forms(&lexemes, &special_flags);
        Self {
            flag_mode,
            case_fallback,
            case_language,
            unique_stem_indices,
            morphology,
            lexemes,
            prefixes,
            suffixes,
            prefix_rules_by_add_edge,
            suffix_rules_by_add_edge,
            prefix_rules_by_flag,
            suffix_rules_by_flag,
            lexeme_indices_by_flag,
            prefix_parent_flags,
            suffix_parent_flags,
            special_flags,
            compound,
            break_patterns,
            sharp_uppercase_forms,
            word_characters,
            replacement_rules,
            keyboard,
            character_maps,
            ignored_characters,
            input_conversions,
            output_conversions,
            full_strip,
            complex_prefixes,
            candidate_index: Arc::new(OnceLock::new()),
        }
    }

    /// Returns extra Unicode scalar values declared as Hunspell word characters.
    ///
    /// This is a tokenization hint. [`Dictionary::contains`] deliberately
    /// receives an already segmented string and therefore does not apply it.
    pub fn word_characters(&self) -> impl Iterator<Item = char> + '_ {
        self.word_characters.iter().copied()
    }

    /// Visits stored stem spellings in deterministic UTF-8 byte order.
    ///
    /// This deliberately does not enumerate affix-derived or compound forms:
    /// their number is not statically bounded by a Hunspell source. Consumers
    /// such as suggestions can use the stable base vocabulary without turning
    /// a lookup dictionary into an unbounded expansion engine.
    pub fn stems(&self) -> impl Iterator<Item = &str> + '_ {
        self.unique_stem_indices
            .iter()
            .map(|index| self.stem_at_index(*index))
    }

    fn stem_at_index(&self, index: u32) -> &str {
        self.lexemes[usize::try_from(index).expect("stem index fits usize")]
            .stem
            .as_ref()
    }

    /// Returns the imported `REP` rules in source order.
    ///
    /// These rules do not affect dictionary recognition. Suggestion clients can
    /// pass them to [`ferrolex_suggest::Suggester::with_replacement_rules`] to
    /// prefer a dictionary's explicit typo corrections.
    #[must_use]
    pub fn replacement_rules(&self) -> &[ReplacementRule] {
        &self.replacement_rules
    }

    /// Returns imported `KEY` and `MAP` data for deterministic suggestion ranking.
    #[must_use]
    pub fn ranking_signals(&self) -> RankingSignals<'_> {
        RankingSignals::new(self.keyboard.as_deref(), &self.character_maps)
    }

    /// Returns whether a stored stem is valid to offer as a suggestion.
    ///
    /// This excludes entries that recognition rejects directly and entries
    /// explicitly marked `NOSUGGEST`, while leaving derived-form generation to
    /// the suggestion layer.
    #[must_use]
    pub fn is_suggestable_stem(&self, stem: &str) -> bool {
        if !self.contains(stem) {
            return false;
        }
        let mut lexemes = self.lexemes_for_stem(stem).peekable();
        lexemes.peek().is_none() || lexemes.any(|lexeme| !self.is_no_suggest(&lexeme.flags))
    }

    fn visit_related_suggestion_forms(
        &self,
        query: &str,
        stem: &str,
        maximum_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        let mut emitted = 0;
        for lexeme in self.lexemes_for_stem(stem) {
            if self.is_forbidden(&lexeme.flags) || self.is_no_suggest(&lexeme.flags) {
                continue;
            }
            let mut states = vec![FormState::new(lexeme)];
            let mut derivations = 0;
            while let Some(state) = states.pop() {
                if state.depth > 0
                    && self.is_accepted_state(&state)
                    && !self.is_no_suggest(state.origin_flags)
                    && !self.is_no_suggest(state.flags)
                {
                    emitted += 1;
                    if emitted > MAX_SUGGESTION_FORMS_PER_SEED || !visitor(&state.form) {
                        return;
                    }
                }
                if state.depth < MAX_AFFIX_CHAIN
                    && self.expand_matching_rules(
                        &state,
                        AffixKind::Prefix,
                        &self.prefixes,
                        &self.prefix_rules_by_flag,
                        &mut states,
                        &mut derivations,
                    )
                {
                    self.expand_matching_rules(
                        &state,
                        AffixKind::Suffix,
                        &self.suffixes,
                        &self.suffix_rules_by_flag,
                        &mut states,
                        &mut derivations,
                    );
                }
            }
        }
        self.visit_compound_suggestion_forms(query, stem, maximum_distance, emitted, visitor);
    }

    fn visit_compound_suggestion_forms(
        &self,
        query: &str,
        stem: &str,
        maximum_distance: usize,
        mut emitted: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        for (boundary, _) in query
            .char_indices()
            .skip(1)
            .take(MAX_SUGGESTION_COMPOUND_SPLITS)
        {
            let (left, right) = query.split_at(boundary);
            for (other, typo_component, stem_is_right) in
                [(left, right, true), (right, left, false)]
            {
                if bounded_osa_distance(stem, typo_component, maximum_distance).is_none()
                    || !self.contains(other)
                {
                    continue;
                }
                let candidate = if stem_is_right {
                    format!("{other}{stem}")
                } else {
                    format!("{stem}{other}")
                };
                if candidate != query && self.contains(&candidate) {
                    emitted += 1;
                    if emitted > MAX_SUGGESTION_FORMS_PER_SEED || !visitor(&candidate) {
                        return;
                    }
                }
            }
        }
    }

    fn lexemes_for_stem(&self, stem: &str) -> impl Iterator<Item = &Lexeme> {
        let range = self.lexeme_index_range(stem);
        self.lexemes[range].iter()
    }

    fn lexeme_index_range(&self, stem: &str) -> std::ops::Range<usize> {
        let start = self
            .lexemes
            .partition_point(|lexeme| lexeme.stem.as_ref() < stem);
        let end =
            self.lexemes[start..].partition_point(|lexeme| lexeme.stem.as_ref() == stem) + start;
        start..end
    }

    /// Applies declared `OCONV` rules to a suggestion spelling.
    #[must_use]
    pub fn normalize_output(&self, word: &str) -> String {
        apply_conversions(word, &self.output_conversions)
    }

    fn normalize_input<'input>(&self, word: &'input str) -> Cow<'input, str> {
        if self.input_conversions.is_empty() && self.ignored_characters.is_empty() {
            return Cow::Borrowed(word);
        }
        let mut normalized = apply_conversions(word, &self.input_conversions);
        if !self.ignored_characters.is_empty() {
            normalized.retain(|character| !self.ignored_characters.contains(&character));
        }
        Cow::Owned(normalized)
    }

    fn case_folded_candidates(&self, word: &str) -> Vec<String> {
        if !self.case_fallback {
            return Vec::new();
        }
        let lower = lowercase_for_language(word, self.case_language);
        let candidates = match case_pattern(word, self.case_language) {
            Some(CasePattern::Initial) => vec![lower],
            Some(CasePattern::Upper) => {
                vec![lower, initial_case_for_language(word, self.case_language)]
            }
            None => Vec::new(),
        };
        candidates
            .into_iter()
            .filter(|candidate| candidate != word)
            .collect()
    }

    fn matches_single_affix_word(&self, word: &str, allow_keep_case: bool) -> bool {
        self.candidate_affix_rules(word).any(|rule| {
            rule.could_generate(word)
                && rule
                    .reverse_apply(word, self.full_strip)
                    .is_some_and(|stem| {
                        self.lexemes_for_stem(&stem).any(|lexeme| {
                            !self.is_forbidden(&lexeme.flags)
                                && has_flag(&lexeme.flags, rule.flag)
                                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                                && self.is_accepted_single_affix(lexeme, rule)
                        })
                    })
        })
    }

    fn is_accepted_single_affix(&self, lexeme: &Lexeme, rule: &AffixRule) -> bool {
        let flags = &rule.continuation_flags;
        let has_circumfix = self
            .special_flags
            .circumfix
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag));
        !self.is_forbidden(flags)
            && !self.requires_affix(flags)
            && !self.is_only_in_compound(&lexeme.flags)
            && !self.is_only_in_compound(flags)
            && !has_circumfix
    }

    fn candidate_affix_rules<'source>(
        &'source self,
        word: &str,
    ) -> impl Iterator<Item = &'source AffixRule> + 'source {
        self.prefix_rules_by_add_edge
            .matching_rules(&self.prefixes, word, AffixKind::Prefix)
            .chain(self.suffix_rules_by_add_edge.matching_rules(
                &self.suffixes,
                word,
                AffixKind::Suffix,
            ))
    }

    fn derived_candidate_indices(&self, word: &str) -> Option<BTreeSet<usize>> {
        let mut candidates = BTreeSet::new();
        let include_empty_add = !self.extend_reverse_derived_candidates(word, &mut candidates);
        if include_empty_add {
            candidates.clear();
        }
        self.extend_derived_candidates(
            (word, AffixKind::Prefix),
            &self.prefixes,
            &self.prefix_rules_by_add_edge,
            &self.prefix_parent_flags,
            include_empty_add,
            &mut candidates,
        )?;
        self.extend_derived_candidates(
            (word, AffixKind::Suffix),
            &self.suffixes,
            &self.suffix_rules_by_add_edge,
            &self.suffix_parent_flags,
            include_empty_add,
            &mut candidates,
        )?;
        Some(candidates)
    }

    fn extend_reverse_derived_candidates(
        &self,
        word: &str,
        candidates: &mut BTreeSet<usize>,
    ) -> bool {
        if self.prefix_rules_by_add_edge.empty_add.is_empty()
            && self.suffix_rules_by_add_edge.empty_add.is_empty()
        {
            return true;
        }

        let mut forms = BTreeSet::from([(word.to_owned(), false)]);
        let mut pending = vec![(word.to_owned(), 0_usize, false)];
        while let Some((form, depth, used_empty_add)) = pending.pop() {
            if depth == MAX_AFFIX_CHAIN {
                continue;
            }
            for rule in self.candidate_affix_rules(&form) {
                let Some(stem) = rule.reverse_apply(&form, self.full_strip) else {
                    continue;
                };
                let used_empty_add = used_empty_add || rule.add.is_empty();
                if used_empty_add {
                    for index in self.lexeme_index_range(&stem) {
                        candidates.insert(index);
                        if candidates.len() > MAX_DERIVED_CANDIDATES_PER_LOOKUP {
                            return false;
                        }
                    }
                }
                let stem = stem.into_owned();
                let state_changed = stem != form || used_empty_add;
                if state_changed && forms.insert((stem.clone(), used_empty_add)) {
                    if forms.len() > MAX_REVERSE_FORMS_PER_LOOKUP {
                        return false;
                    }
                    pending.push((stem, depth + 1, used_empty_add));
                }
            }
        }
        true
    }

    fn extend_derived_candidates(
        &self,
        query: (&str, AffixKind),
        rules: &[AffixRule],
        rules_by_add_edge: &AffixRuleIndex,
        parent_flags: &BTreeMap<Flag, BTreeSet<Flag>>,
        include_empty_add: bool,
        candidates: &mut BTreeSet<usize>,
    ) -> Option<()> {
        let (word, kind) = query;
        for rule in rules_by_add_edge
            .matching_rules(rules, word, kind)
            .filter(|rule| include_empty_add || !rule.add.is_empty())
            .filter(|rule| rule.could_generate(word))
        {
            for flag in origin_flags_for(rule.flag, parent_flags) {
                if let Some(indices) = self.lexeme_indices_by_flag.get(&flag) {
                    for index in indices {
                        candidates.insert(*index);
                        if candidates.len() > MAX_DERIVED_CANDIDATES_PER_LOOKUP {
                            return None;
                        }
                    }
                }
            }
        }
        Some(())
    }

    fn matches_derived_word<'source>(
        &'source self,
        lexeme: &'source Lexeme,
        word: &str,
        allow_keep_case: bool,
    ) -> bool {
        if self.is_forbidden(&lexeme.flags)
            || (!allow_keep_case && self.is_keep_case(&lexeme.flags))
        {
            return false;
        }
        let mut states = vec![FormState::new(lexeme)];
        let mut derivations = 0;

        while let Some(state) = states.pop() {
            if state.depth > 0 && state.form == word && self.is_accepted_state(&state) {
                return true;
            }
            if state.depth == MAX_AFFIX_CHAIN {
                continue;
            }
            if !self.expand_matching_rules(
                &state,
                AffixKind::Prefix,
                &self.prefixes,
                &self.prefix_rules_by_flag,
                &mut states,
                &mut derivations,
            ) || !self.expand_matching_rules(
                &state,
                AffixKind::Suffix,
                &self.suffixes,
                &self.suffix_rules_by_flag,
                &mut states,
                &mut derivations,
            ) {
                return false;
            }
        }
        false
    }

    fn compound_rule_is_allowed(&self, rule: &AffixRule, position: CompoundPosition) -> bool {
        let permit = self
            .compound
            .permit
            .as_ref()
            .is_some_and(|flag| has_flag(&rule.continuation_flags, *flag));
        match position {
            CompoundPosition::Begin => rule.kind == AffixKind::Prefix || permit,
            CompoundPosition::Middle => permit,
            CompoundPosition::End => rule.kind == AffixKind::Suffix || permit,
        }
    }

    fn expand_matching_rules<'source>(
        &'source self,
        state: &FormState<'source>,
        kind: AffixKind,
        rules: &'source [AffixRule],
        rule_indices_by_flag: &BTreeMap<Flag, Vec<usize>>,
        states: &mut Vec<FormState<'source>>,
        derivations: &mut usize,
    ) -> bool {
        let flags = state.flags_for(kind);
        for flag in flags {
            let Some(rule_indices) = rule_indices_by_flag.get(flag) else {
                continue;
            };
            for index in rule_indices {
                let rule = &rules[*index];
                if !state.can_apply(rule, self.complex_prefixes) {
                    continue;
                }
                if let Some(form) = rule.apply(&state.form, self.full_strip) {
                    if *derivations == MAX_DERIVATIONS_PER_LEXEME {
                        return false;
                    }
                    *derivations += 1;
                    states.push(state.apply(rule, form, &self.special_flags));
                }
            }
        }
        true
    }

    fn is_accepted_state(&self, state: &FormState<'_>) -> bool {
        !self.is_forbidden(state.flags)
            && !self.requires_affix(state.flags)
            && !self.is_only_in_compound(state.origin_flags)
            && !self.is_only_in_compound(state.flags)
            && state.has_complete_circumfix()
    }

    fn is_accepted_compound_state(&self, state: &FormState<'_>) -> bool {
        !self.is_forbidden(state.flags)
            && !self.is_compound_forbidden(state.origin_flags)
            && !self.is_compound_forbidden(state.flags)
            && !self.requires_affix(state.flags)
            && state.has_complete_circumfix()
    }

    fn is_forbidden(&self, flags: &[Flag]) -> bool {
        self.special_flags
            .forbidden_word
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn is_compound_forbidden(&self, flags: &[Flag]) -> bool {
        self.compound
            .forbid
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn requires_affix(&self, flags: &[Flag]) -> bool {
        self.special_flags
            .need_affix
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn is_only_in_compound(&self, flags: &[Flag]) -> bool {
        self.special_flags
            .only_in_compound
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn is_no_suggest(&self, flags: &[Flag]) -> bool {
        self.special_flags
            .no_suggest
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn is_keep_case(&self, flags: &[Flag]) -> bool {
        self.special_flags
            .keep_case
            .as_ref()
            .is_some_and(|flag| has_flag(flags, *flag))
    }

    fn matches_simple_compound(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.compound.flag.is_none()
            && self.compound.rules.is_empty()
            && (self.compound.begin.is_none() || self.compound.end.is_none())
        {
            return false;
        }
        if self.compound.check_replacement
            && self.matches_noncompound_replacement(word, allow_keep_case)
        {
            return false;
        }
        // Retain at most the bounded number of split positions. Building an
        // index for an arbitrarily long untrusted query would defeat the
        // compound-evaluation limit before it can reject the query.
        let mut boundaries = word
            .char_indices()
            .take(MAX_COMPOUND_SCALARS.saturating_add(1))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if boundaries.len() > MAX_COMPOUND_SCALARS {
            return false;
        }
        boundaries.push(word.len());

        if self.matches_simple_compound_with_triples(word, &boundaries, allow_keep_case, false) {
            return true;
        }
        self.compound.check_triple
            && self.compound.simplified_triple
            && self.matches_simplified_triple_compound(word, allow_keep_case)
            || self.matches_compound_pattern_replacements(word, allow_keep_case)
    }

    fn matches_simple_compound_with_triples(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        self.compound.flag.as_ref().is_some_and(|flag| {
            self.matches_compound_pattern(
                word,
                boundaries,
                None,
                Some(flag),
                allow_keep_case,
                allow_boundary_triples,
            )
        }) || self.compound.rules.iter().any(|rule| {
            rule.patterns.iter().any(|pattern| {
                self.matches_compound_pattern(
                    word,
                    boundaries,
                    Some(pattern),
                    None,
                    allow_keep_case,
                    allow_boundary_triples,
                )
            })
        }) || self.matches_positioned_compound(
            word,
            boundaries,
            allow_keep_case,
            allow_boundary_triples,
        )
    }

    fn matches_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: Option<&[Flag]>,
        generic_flag: Option<&Flag>,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        if let Some(pattern) = pattern {
            return self.matches_fixed_compound_pattern(
                word,
                boundaries,
                pattern,
                allow_keep_case,
                allow_boundary_triples,
            );
        }
        let Some(flag) = generic_flag else {
            return false;
        };

        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for component_count in 1..boundaries.len() {
            let next = self.extend_compound_components(
                word,
                boundaries,
                &reachable,
                *flag,
                allow_keep_case,
                allow_boundary_triples,
            );
            if component_count >= 2
                && next.last() == Some(&true)
                && self.compound_component_count_is_allowed(word, component_count)
            {
                return true;
            }
            if self.compound_component_count_cannot_continue(component_count) {
                return false;
            }
            reachable = next;
        }
        false
    }

    fn matches_fixed_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: &[Flag],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        if pattern.len() < 2 {
            return false;
        }
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for flag in pattern {
            let next = self.extend_compound_components(
                word,
                boundaries,
                &reachable,
                *flag,
                allow_keep_case,
                allow_boundary_triples,
            );
            if next.iter().all(|reachable| !reachable) {
                return false;
            }
            reachable = next;
        }
        reachable.last() == Some(&true)
            && self.compound_component_count_is_allowed(word, pattern.len())
    }

    fn extend_compound_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        flag: Flag,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.compound_boundary_is_allowed(
                    word,
                    boundaries[start],
                    boundaries[end],
                    allow_boundary_triples,
                ) && self.matches_compound_component(
                    candidate,
                    flag,
                    end + 1 == boundaries.len(),
                    allow_keep_case,
                ) {
                    next[end] = true;
                }
            }
        }
        next
    }

    fn matches_compound_component(
        &self,
        word: &str,
        required_flag: Flag,
        is_final_component: bool,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && (is_final_component || !self.is_compound_forbidden(&lexeme.flags))
                && has_flag(&lexeme.flags, required_flag)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        })
    }

    fn matches_positioned_compound(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> bool {
        let (Some(begin), Some(end)) = (&self.compound.begin, &self.compound.end) else {
            return false;
        };
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        reachable = self.extend_positioned_components(
            word,
            boundaries,
            &reachable,
            *begin,
            CompoundPosition::Begin,
            allow_keep_case,
            allow_boundary_triples,
        );
        for component_count in 2..boundaries.len() {
            let terminal = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                *end,
                CompoundPosition::End,
                allow_keep_case,
                allow_boundary_triples,
            );
            if terminal.last() == Some(&true)
                && self.compound_component_count_is_allowed(word, component_count)
            {
                return true;
            }
            if self.compound_component_count_cannot_continue(component_count) {
                return false;
            }
            let Some(middle) = self.compound.middle.as_ref() else {
                return false;
            };
            reachable = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                *middle,
                CompoundPosition::Middle,
                allow_keep_case,
                allow_boundary_triples,
            );
        }
        false
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "compound position, casing, and triple policy describe one bounded transition"
    )]
    fn extend_positioned_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
        allow_boundary_triples: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.compound_boundary_is_allowed(
                    word,
                    boundaries[start],
                    boundaries[end],
                    allow_boundary_triples,
                ) && self.matches_positioned_component(
                    candidate,
                    position_flag,
                    position,
                    allow_keep_case,
                ) {
                    next[end] = true;
                }
            }
        }
        next
    }

    fn matches_positioned_component(
        &self,
        word: &str,
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && (position == CompoundPosition::End || !self.is_compound_forbidden(&lexeme.flags))
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                && (has_flag(&lexeme.flags, position_flag)
                    || self
                        .compound
                        .flag
                        .as_ref()
                        .is_some_and(|flag| has_flag(&lexeme.flags, *flag)))
        }) || self.matches_one_affix_compound_component(
            word,
            position_flag,
            position,
            allow_keep_case,
        )
    }

    fn matches_one_affix_compound_component(
        &self,
        word: &str,
        position_flag: Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.candidate_affix_rules(word)
            .filter(|rule| self.compound_rule_is_allowed(rule, position))
            .any(|rule| {
                rule.reverse_apply(word, self.full_strip)
                    .is_some_and(|stem| {
                        self.lexemes_for_stem(&stem).any(|lexeme| {
                            !self.is_forbidden(&lexeme.flags)
                                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                                && has_flag(&lexeme.flags, rule.flag)
                                && (has_flag(&lexeme.flags, position_flag)
                                    || self
                                        .compound
                                        .flag
                                        .as_ref()
                                        .is_some_and(|flag| has_flag(&lexeme.flags, *flag)))
                                && self.is_accepted_compound_state(&FormState::new(lexeme).apply(
                                    rule,
                                    word.to_owned(),
                                    &self.special_flags,
                                ))
                        })
                    })
            })
    }

    fn compound_component_count_is_allowed(&self, word: &str, component_count: usize) -> bool {
        let Some(maximum_words) = self.compound.maximum_words else {
            return true;
        };
        if component_count <= maximum_words {
            return true;
        }
        self.compound.syllable_limit.as_ref().is_some_and(|limit| {
            word.chars()
                .filter(|character| limit.vowels.contains(character))
                .take(limit.maximum.saturating_add(1))
                .count()
                <= limit.maximum
        })
    }

    fn compound_component_count_cannot_continue(&self, component_count: usize) -> bool {
        self.compound.maximum_words.is_some_and(|maximum_words| {
            component_count >= maximum_words && self.compound.syllable_limit.is_none()
        })
    }

    fn compound_boundary_is_allowed(
        &self,
        word: &str,
        start: usize,
        end: usize,
        allow_boundary_triples: bool,
    ) -> bool {
        let component = &word[start..end];
        if self.compound.check_case
            && start != 0
            && component.chars().next().is_some_and(char::is_uppercase)
        {
            return false;
        }
        if self.compound.check_duplicate
            && start >= component.len()
            && word[..start].ends_with(component)
        {
            return false;
        }
        if self.compound.check_triple
            && !allow_boundary_triples
            && has_triple_at_compound_boundary(word, start)
        {
            return false;
        }
        if end == word.len()
            && self.compound.force_uppercase.as_ref().is_some_and(|flag| {
                self.lexemes_for_stem(component)
                    .any(|lexeme| has_flag(&lexeme.flags, *flag))
            })
            && !word.chars().next().is_some_and(char::is_uppercase)
        {
            return false;
        }
        !self.compound_pattern_forbids(word, start, component)
    }

    fn matches_simplified_triple_compound(&self, word: &str, allow_keep_case: bool) -> bool {
        for (boundary, character) in word.char_indices().skip(1) {
            let Some(previous) = word[..boundary].chars().last() else {
                continue;
            };
            if character != previous {
                continue;
            }
            let mut expanded = String::with_capacity(word.len() + previous.len_utf8());
            expanded.push_str(&word[..boundary]);
            expanded.push(previous);
            expanded.push_str(&word[boundary..]);
            let Some(boundaries) = compound_boundaries(&expanded) else {
                continue;
            };
            if self.matches_simple_compound_with_triples(
                &expanded,
                &boundaries,
                allow_keep_case,
                true,
            ) {
                return true;
            }
        }
        false
    }

    fn matches_compound_pattern_replacements(&self, word: &str, allow_keep_case: bool) -> bool {
        self.compound.patterns.iter().any(|pattern| {
            let Some(replacement) = pattern.replacement.as_deref() else {
                return false;
            };
            word.match_indices(replacement)
                .take(MAX_COMPOUND_PATTERN_REPLACEMENT_VARIANTS)
                .any(|(start, _)| {
                    let end = start + replacement.len();
                    let mut expanded = String::with_capacity(
                        word.len()
                            .saturating_add(pattern.ending.len())
                            .saturating_add(pattern.beginning.len())
                            .saturating_sub(replacement.len()),
                    );
                    expanded.push_str(&word[..start]);
                    expanded.push_str(&pattern.ending);
                    expanded.push_str(&pattern.beginning);
                    expanded.push_str(&word[end..]);
                    compound_boundaries(&expanded).is_some_and(|boundaries| {
                        self.matches_simple_compound_with_triples(
                            &expanded,
                            &boundaries,
                            allow_keep_case,
                            false,
                        )
                    })
                })
        })
    }

    fn compound_pattern_forbids(&self, word: &str, start: usize, right: &str) -> bool {
        self.compound.patterns.iter().any(|pattern| {
            pattern.replacement.is_none()
                && pattern.ending.as_ref() != "0"
                && word[..start].ends_with(pattern.ending.as_ref())
                && right.starts_with(pattern.beginning.as_ref())
                && pattern.ending_flag.as_ref().is_none_or(|flag| {
                    self.lexemes_for_stem(&word[..start])
                        .any(|lexeme| has_flag(&lexeme.flags, *flag))
                })
                && pattern.beginning_flag.as_ref().is_none_or(|flag| {
                    self.lexemes_for_stem(right)
                        .any(|lexeme| has_flag(&lexeme.flags, *flag))
                })
        })
    }

    fn matches_noncompound_replacement(&self, word: &str, allow_keep_case: bool) -> bool {
        self.replacement_rules.iter().any(|rule| {
            word.match_indices(rule.from()).any(|(start, _)| {
                let end = start + rule.from().len();
                if (rule.at_word_start() && start != 0) || (rule.at_word_end() && end != word.len())
                {
                    return false;
                }
                let mut corrected = String::with_capacity(
                    word.len()
                        .saturating_add(rule.to().len())
                        .saturating_sub(rule.from().len()),
                );
                corrected.push_str(&word[..start]);
                corrected.push_str(rule.to());
                corrected.push_str(&word[end..]);
                self.matches_noncompound_word(&corrected, allow_keep_case)
            })
        })
    }

    fn matches_noncompound_word(&self, word: &str, allow_keep_case: bool) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && !self.requires_affix(&lexeme.flags)
                && !self.is_only_in_compound(&lexeme.flags)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        }) || self.matches_single_affix_word(word, allow_keep_case)
    }

    fn matches_break_word(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.break_patterns.is_empty() || word.chars().count() > MAX_COMPOUND_SCALARS {
            return false;
        }
        self.break_patterns.iter().any(|pattern| {
            if pattern.at_start {
                return word
                    .strip_prefix(pattern.text.as_ref())
                    .is_some_and(|rest| {
                        !rest.is_empty() && self.matches_without_break(rest, allow_keep_case)
                    });
            }
            if pattern.at_end {
                return word
                    .strip_suffix(pattern.text.as_ref())
                    .is_some_and(|rest| {
                        !rest.is_empty() && self.matches_without_break(rest, allow_keep_case)
                    });
            }
            word.match_indices(pattern.text.as_ref()).any(|(start, _)| {
                let end = start + pattern.text.len();
                let (left, right) = (&word[..start], &word[end..]);
                !left.is_empty()
                    && !right.is_empty()
                    && self.matches_without_break(left, allow_keep_case)
                    && self.matches_without_break(right, allow_keep_case)
            })
        })
    }
}

impl CandidateSource for HunspellDictionary {
    fn visit_candidates(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        for stem in self.stems() {
            if !visitor(stem) {
                break;
            }
        }
    }

    fn contains_candidate(&self, word: &str) -> bool {
        self.contains(word)
    }

    fn visit_nearby_candidates(
        &self,
        query: &[char],
        max_edit_distance: usize,
        max_word_scalars: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.candidate_index
            .get_or_init(|| CandidateIndex::new(self.stems(), max_word_scalars))
            .visit_nearby(query, max_edit_distance, max_word_scalars, visitor);
    }

    fn is_suggestion_candidate(&self, candidate: &str) -> bool {
        self.is_suggestable_stem(candidate)
    }

    fn visit_related_candidates(
        &self,
        query: &str,
        seed: &str,
        max_edit_distance: usize,
        visitor: &mut dyn FnMut(&str) -> bool,
    ) {
        self.visit_related_suggestion_forms(query, seed, max_edit_distance, visitor);
    }

    fn visit_related_seeds(&self, visitor: &mut dyn FnMut(&str) -> bool) {
        self.visit_candidates(visitor);
    }
}

fn bounded_osa_distance(left: &str, right: &str, maximum: usize) -> Option<usize> {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len().abs_diff(right.len()) > maximum {
        return None;
    }
    let mut previous_previous = vec![0; right.len() + 1];
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.iter().enumerate() {
        let mut current = vec![left_index + 1; right.len() + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            let cost = usize::from(left_character != right_character);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + cost);
            if left_index > 0
                && right_index > 0
                && *left_character == right[right_index - 1]
                && left[left_index - 1] == *right_character
            {
                current[right_index + 1] =
                    current[right_index + 1].min(previous_previous[right_index - 1] + 1);
            }
        }
        previous_previous = previous;
        previous = current;
    }
    (previous[right.len()] <= maximum).then_some(previous[right.len()])
}

fn sharp_uppercase_forms(lexemes: &[Lexeme], special_flags: &SpecialFlags) -> BTreeSet<Box<str>> {
    if !special_flags.check_sharps {
        return BTreeSet::new();
    }
    let Some(keep_case) = special_flags.keep_case.as_ref() else {
        return BTreeSet::new();
    };
    lexemes
        .iter()
        .filter(|lexeme| has_flag(&lexeme.flags, *keep_case) && lexeme.stem.contains('ß'))
        .map(|lexeme| Box::<str>::from(lexeme.stem.to_uppercase()))
        .collect()
}

fn unique_stem_indices(lexemes: &[Lexeme]) -> Vec<u32> {
    let mut indices = Vec::new();
    for (index, lexeme) in lexemes.iter().enumerate() {
        if index == 0 || lexemes[index - 1].stem != lexeme.stem {
            indices.push(u32::try_from(index).expect("dictionary entry count is bounded"));
        }
    }
    indices
}

fn rule_indices_by_flag(rules: &[AffixRule]) -> BTreeMap<Flag, Vec<usize>> {
    let mut indices = BTreeMap::<Flag, Vec<usize>>::new();
    for (index, rule) in rules.iter().enumerate() {
        indices.entry(rule.flag).or_default().push(index);
    }
    indices
}

fn lexeme_indices_by_flag(lexemes: &[Lexeme]) -> BTreeMap<Flag, Vec<usize>> {
    let mut indices = BTreeMap::<Flag, Vec<usize>>::new();
    for (index, lexeme) in lexemes.iter().enumerate() {
        for flag in &lexeme.flags {
            indices.entry(*flag).or_default().push(index);
        }
    }
    indices
}

fn parent_flags_by_continuation(rules: &[AffixRule]) -> BTreeMap<Flag, BTreeSet<Flag>> {
    let mut parents = BTreeMap::<Flag, BTreeSet<Flag>>::new();
    for rule in rules {
        for continuation in &rule.continuation_flags {
            parents.entry(*continuation).or_default().insert(rule.flag);
        }
    }
    parents
}

fn origin_flags_for(
    terminal_flag: Flag,
    parent_flags: &BTreeMap<Flag, BTreeSet<Flag>>,
) -> BTreeSet<Flag> {
    let mut origins = BTreeSet::from([terminal_flag]);
    let mut pending = vec![terminal_flag];
    while let Some(flag) = pending.pop() {
        if let Some(parents) = parent_flags.get(&flag) {
            for parent in parents {
                if origins.insert(*parent) {
                    pending.push(*parent);
                }
            }
        }
    }
    origins
}

#[derive(Clone, Debug)]
struct Lexeme {
    stem: Box<str>,
    flags: FlagSet,
    morphology: Morphology,
}

type Morphology = Box<[MorphologyId]>;
type FlagSet = Box<[Flag]>;

fn has_flag(flags: &[Flag], flag: Flag) -> bool {
    flags.binary_search(&flag).is_ok()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MorphologyId(u32);

/// Stores each morphology field only once while keeping stable compact IDs in
/// dictionary entries and affix rules.
#[derive(Clone, Debug, Default)]
struct MorphologyTable {
    ids: BTreeMap<Box<str>, MorphologyId>,
}

impl MorphologyTable {
    fn intern(&mut self, field: &str) -> Option<MorphologyId> {
        if let Some(id) = self.ids.get(field) {
            return Some(*id);
        }
        if self.ids.len() >= MAX_MORPHOLOGY_STRINGS {
            return None;
        }
        let id = MorphologyId(u32::try_from(self.ids.len()).expect("morphology ID is bounded"));
        self.ids.insert(Box::from(field), id);
        Some(id)
    }

    fn contains(&self, id: MorphologyId) -> bool {
        usize::try_from(id.0).is_ok_and(|index| index < self.ids.len())
    }

    fn values_by_id(&self) -> Vec<&str> {
        let mut values = vec![""; self.ids.len()];
        for (value, id) in &self.ids {
            values[usize::try_from(id.0).expect("morphology ID fits usize")] = value;
        }
        values
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Flag(u64);

fn encode_text_flag(value: &str) -> Option<u64> {
    let mut characters = value.chars();
    let first = u64::from(u32::from(characters.next()?));
    let second = characters
        .next()
        .map_or(0, |character| u64::from(u32::from(character)) + 1);
    characters
        .next()
        .is_none()
        .then_some((first << 32) | second)
}

fn decode_text_flag(value: u64) -> Option<String> {
    let (first, second) = decode_text_flag_chars(value)?;
    let mut decoded = String::with_capacity(8);
    decoded.push(first);
    if let Some(second) = second {
        decoded.push(second);
    }
    Some(decoded)
}

fn decode_text_flag_chars(value: u64) -> Option<(char, Option<char>)> {
    let first = char::from_u32(u32::try_from(value >> 32).ok()?)?;
    let encoded_second = u32::try_from(value & u64::from(u32::MAX)).ok()?;
    let second = (encoded_second != 0)
        .then(|| char::from_u32(encoded_second - 1))
        .flatten();
    (encoded_second == 0 || second.is_some()).then_some((first, second))
}

impl Flag {
    fn is_valid_for(self, mode: FlagMode) -> bool {
        match mode {
            FlagMode::Numeric => u32::try_from(self.0).is_ok(),
            FlagMode::Unicode => decode_text_flag_chars(self.0).is_some_and(|(first, second)| {
                !is_variation_selector(first) && second.is_none_or(is_variation_selector)
            }),
            FlagMode::Long => {
                decode_text_flag_chars(self.0).is_some_and(|(_, second)| second.is_some())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FlagMode {
    #[default]
    Unicode,
    Long,
    Numeric,
}

/// Language-specific casing used by Hunspell's capitalization fallback.
///
/// Hunspell distinguishes Turkish, Azeri, and Crimean Tatar for dotted and
/// dotless `I`; every other `LANG` value uses Unicode's default casing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CaseLanguage {
    #[default]
    Default,
    Turkic,
}

impl CaseLanguage {
    fn from_lang(value: &str) -> Self {
        match value
            .split(['_', '-'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "az" | "crh" | "tr" => Self::Turkic,
            _ => Self::Default,
        }
    }
}

#[derive(Clone, Copy)]
enum CasePattern {
    Initial,
    Upper,
}

fn case_pattern(word: &str, language: CaseLanguage) -> Option<CasePattern> {
    let mut cased = word
        .chars()
        .filter(|character| is_cased(*character, language));
    let first = cased.next()?;
    if is_uppercase(first, language) && cased.all(|character| is_uppercase(character, language)) {
        return Some(CasePattern::Upper);
    }
    if is_uppercase(first, language) && cased.all(|character| is_lowercase(character, language)) {
        return Some(CasePattern::Initial);
    }
    None
}

fn is_cased(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        match character {
            'I' | 'İ' | 'i' | 'ı' => return true,
            _ => {}
        }
    }
    !character.to_lowercase().eq(character.to_uppercase())
}

fn is_uppercase(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        return matches!(character, 'I' | 'İ')
            || (!matches!(character, 'i' | 'ı') && lowercase_changes(character));
    }
    lowercase_changes(character)
}

fn is_lowercase(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        return matches!(character, 'i' | 'ı')
            || (!matches!(character, 'I' | 'İ') && uppercase_changes(character));
    }
    uppercase_changes(character)
}

fn lowercase_changes(character: char) -> bool {
    let mut lowercase = character.to_lowercase();
    lowercase.next() != Some(character) || lowercase.next().is_some()
}

fn uppercase_changes(character: char) -> bool {
    let mut uppercase = character.to_uppercase();
    uppercase.next() != Some(character) || uppercase.next().is_some()
}

fn lowercase_for_language(word: &str, language: CaseLanguage) -> String {
    let mut result = String::with_capacity(word.len());
    for character in word.chars() {
        push_lowercase(&mut result, character, language);
    }
    result
}

fn initial_case_for_language(word: &str, language: CaseLanguage) -> String {
    let Some((index, first)) = word.char_indices().next() else {
        return String::new();
    };
    let mut result = String::with_capacity(word.len());
    push_uppercase(&mut result, first, language);
    result.push_str(&lowercase_for_language(
        &word[index + first.len_utf8()..],
        language,
    ));
    result
}

fn push_lowercase(result: &mut String, character: char, language: CaseLanguage) {
    if language == CaseLanguage::Turkic {
        match character {
            'I' => return result.push('ı'),
            'İ' => return result.push('i'),
            _ => {}
        }
    }
    result.extend(character.to_lowercase());
}

fn push_uppercase(result: &mut String, character: char, language: CaseLanguage) {
    if language == CaseLanguage::Turkic {
        match character {
            'i' => return result.push('İ'),
            'ı' => return result.push('I'),
            _ => {}
        }
    }
    result.extend(character.to_uppercase());
}

#[derive(Clone, Debug)]
struct InputConversion {
    from: Box<str>,
    to: Box<str>,
    at_word_start: bool,
    at_word_end: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AffixKind {
    Prefix,
    Suffix,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CompoundPosition {
    Begin,
    Middle,
    End,
}

#[derive(Clone, Debug)]
struct AffixRule {
    id: usize,
    kind: AffixKind,
    flag: Flag,
    strip: Box<str>,
    add: Box<str>,
    condition: Condition,
    cross_product: bool,
    continuation_flags: FlagSet,
    morphology: Morphology,
}

#[derive(Clone, Debug, Default)]
struct AffixRuleIndex {
    empty_add: Vec<usize>,
    by_add_edge: BTreeMap<char, Vec<usize>>,
}

impl AffixRuleIndex {
    fn new(rules: &[AffixRule], kind: AffixKind) -> Self {
        let mut index = Self::default();
        for (rule_index, rule) in rules.iter().enumerate() {
            let edge = match kind {
                AffixKind::Prefix => rule.add.chars().next(),
                AffixKind::Suffix => rule.add.chars().next_back(),
            };
            if let Some(edge) = edge {
                index.by_add_edge.entry(edge).or_default().push(rule_index);
            } else {
                index.empty_add.push(rule_index);
            }
        }
        index
    }

    fn matching_rules<'source>(
        &'source self,
        rules: &'source [AffixRule],
        word: &str,
        kind: AffixKind,
    ) -> impl Iterator<Item = &'source AffixRule> + 'source {
        let edge = match kind {
            AffixKind::Prefix => word.chars().next(),
            AffixKind::Suffix => word.chars().next_back(),
        };
        let matching = edge
            .and_then(|edge| self.by_add_edge.get(&edge))
            .map_or(&[][..], Vec::as_slice);
        self.empty_add
            .iter()
            .chain(matching)
            .map(|index| &rules[*index])
    }
}

impl AffixRule {
    fn could_generate(&self, word: &str) -> bool {
        match self.kind {
            AffixKind::Prefix => word.starts_with(self.add.as_ref()),
            AffixKind::Suffix => word.ends_with(self.add.as_ref()),
        }
    }

    fn apply(&self, stem: &str, full_strip: bool) -> Option<String> {
        if !self.condition.matches(stem, self.kind) {
            return None;
        }

        match self.kind {
            AffixKind::Prefix => stem
                .strip_prefix(self.strip.as_ref())
                .and_then(|remaining| {
                    (full_strip || !remaining.is_empty()).then(|| {
                        let mut form = String::with_capacity(self.add.len() + remaining.len());
                        form.push_str(&self.add);
                        form.push_str(remaining);
                        form
                    })
                }),
            AffixKind::Suffix => stem
                .strip_suffix(self.strip.as_ref())
                .and_then(|remaining| {
                    (full_strip || !remaining.is_empty()).then(|| {
                        let mut form = String::with_capacity(remaining.len() + self.add.len());
                        form.push_str(remaining);
                        form.push_str(&self.add);
                        form
                    })
                }),
        }
    }

    fn reverse_apply<'form>(&self, form: &'form str, full_strip: bool) -> Option<Cow<'form, str>> {
        let (remaining, stem) = match self.kind {
            AffixKind::Prefix => {
                let remaining = form.strip_prefix(self.add.as_ref())?;
                let stem = if self.strip.is_empty() {
                    Cow::Borrowed(remaining)
                } else {
                    let mut stem = String::with_capacity(self.strip.len() + remaining.len());
                    stem.push_str(&self.strip);
                    stem.push_str(remaining);
                    Cow::Owned(stem)
                };
                (remaining, stem)
            }
            AffixKind::Suffix => {
                let remaining = form.strip_suffix(self.add.as_ref())?;
                let stem = if self.strip.is_empty() {
                    Cow::Borrowed(remaining)
                } else {
                    let mut stem = String::with_capacity(remaining.len() + self.strip.len());
                    stem.push_str(remaining);
                    stem.push_str(&self.strip);
                    Cow::Owned(stem)
                };
                (remaining, stem)
            }
        };
        (full_strip || !remaining.is_empty())
            .then_some(())
            .filter(|()| self.condition.matches(&stem, self.kind))
            .map(|()| stem)
    }
}

#[derive(Clone, Debug)]
struct FormState<'source> {
    form: String,
    flags: &'source [Flag],
    origin_flags: &'source [Flag],
    depth: usize,
    prefix_count: usize,
    suffix_count: usize,
    last_kind: Option<AffixKind>,
    last_cross_product: bool,
    used_rules: [usize; MAX_AFFIX_CHAIN],
    circumfix_prefix: bool,
    circumfix_suffix: bool,
}

impl<'source> FormState<'source> {
    fn new(lexeme: &'source Lexeme) -> Self {
        Self {
            form: lexeme.stem.to_string(),
            flags: &lexeme.flags,
            origin_flags: &lexeme.flags,
            depth: 0,
            prefix_count: 0,
            suffix_count: 0,
            last_kind: None,
            last_cross_product: true,
            used_rules: [usize::MAX; MAX_AFFIX_CHAIN],
            circumfix_prefix: false,
            circumfix_suffix: false,
        }
    }

    fn can_apply(&self, rule: &AffixRule, complex_prefixes: bool) -> bool {
        !self.used_rules[..self.depth].contains(&rule.id)
            && match rule.kind {
                // COMPLEXPREFIXES permits a second prefix. Prefixes still
                // precede every suffix so the derived form remains bounded.
                AffixKind::Prefix => {
                    self.prefix_count < if complex_prefixes { 2 } else { 1 }
                        && self.suffix_count == 0
                }
                // Continuation classes may supply one additional suffix.
                AffixKind::Suffix => self.suffix_count < 2,
            }
            && match self.last_kind {
                None => has_flag(self.flags, rule.flag),
                Some(kind) if kind == rule.kind => has_flag(self.flags, rule.flag),
                Some(_) => {
                    self.last_cross_product
                        && rule.cross_product
                        && has_flag(self.origin_flags, rule.flag)
                }
            }
    }

    fn flags_for(&self, kind: AffixKind) -> &[Flag] {
        match self.last_kind {
            Some(previous_kind) if previous_kind != kind => self.origin_flags,
            Some(_) | None => self.flags,
        }
    }

    fn apply(&self, rule: &'source AffixRule, form: String, special_flags: &SpecialFlags) -> Self {
        let circumfix = special_flags
            .circumfix
            .as_ref()
            .is_some_and(|flag| has_flag(&rule.continuation_flags, *flag));
        let mut used_rules = self.used_rules;
        used_rules[self.depth] = rule.id;
        Self {
            form,
            flags: &rule.continuation_flags,
            origin_flags: self.origin_flags,
            depth: self.depth + 1,
            prefix_count: self.prefix_count + usize::from(rule.kind == AffixKind::Prefix),
            suffix_count: self.suffix_count + usize::from(rule.kind == AffixKind::Suffix),
            last_kind: Some(rule.kind),
            last_cross_product: rule.cross_product,
            used_rules,
            circumfix_prefix: self.circumfix_prefix
                || (circumfix && rule.kind == AffixKind::Prefix),
            circumfix_suffix: self.circumfix_suffix
                || (circumfix && rule.kind == AffixKind::Suffix),
        }
    }

    fn has_complete_circumfix(&self) -> bool {
        self.circumfix_prefix == self.circumfix_suffix
    }
}

#[derive(Clone, Debug, Default)]
struct SpecialFlags {
    circumfix: Option<Flag>,
    forbidden_word: Option<Flag>,
    keep_case: Option<Flag>,
    need_affix: Option<Flag>,
    only_in_compound: Option<Flag>,
    no_suggest: Option<Flag>,
    check_sharps: bool,
}

#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each imported Hunspell marker is an independent recognition safeguard"
)]
struct CompoundConfig {
    flag: Option<Flag>,
    begin: Option<Flag>,
    middle: Option<Flag>,
    end: Option<Flag>,
    permit: Option<Flag>,
    forbid: Option<Flag>,
    force_uppercase: Option<Flag>,
    minimum_length: usize,
    maximum_words: Option<usize>,
    check_duplicate: bool,
    check_replacement: bool,
    check_case: bool,
    check_triple: bool,
    simplified_triple: bool,
    patterns: Vec<CompoundPattern>,
    syllable_limit: Option<CompoundSyllableLimit>,
    rules: Vec<CompoundRule>,
}

impl Default for CompoundConfig {
    fn default() -> Self {
        Self {
            flag: None,
            begin: None,
            middle: None,
            end: None,
            permit: None,
            forbid: None,
            force_uppercase: None,
            minimum_length: 3,
            maximum_words: None,
            check_duplicate: false,
            check_replacement: false,
            check_case: false,
            check_triple: false,
            simplified_triple: false,
            patterns: Vec::new(),
            syllable_limit: None,
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct CompoundPattern {
    ending: Box<str>,
    ending_flag: Option<Flag>,
    beginning: Box<str>,
    beginning_flag: Option<Flag>,
    replacement: Option<Box<str>>,
}

#[derive(Clone, Debug)]
struct CompoundSyllableLimit {
    maximum: usize,
    vowels: BTreeSet<char>,
}

#[derive(Clone, Debug)]
struct CompoundRule {
    patterns: Vec<Vec<Flag>>,
}

#[derive(Clone, Debug)]
struct Condition {
    atoms: Vec<ConditionAtom>,
    not_preceded_by: Option<ConditionAtom>,
    anchored_at_start: bool,
}

impl Condition {
    fn empty() -> Self {
        Self {
            atoms: Vec::new(),
            not_preceded_by: None,
            anchored_at_start: false,
        }
    }

    fn matches(&self, stem: &str, kind: AffixKind) -> bool {
        if kind == AffixKind::Prefix || self.anchored_at_start {
            let mut characters = stem.chars();
            return self.atoms.iter().all(|atom| {
                characters
                    .next()
                    .is_some_and(|character| atom.matches(character))
            });
        }

        let mut characters = stem.chars().rev();
        self.atoms.iter().rev().all(|atom| {
            characters
                .next()
                .is_some_and(|character| atom.matches(character))
        }) && self.not_preceded_by.as_ref().is_none_or(|atom| {
            characters
                .next()
                .is_none_or(|character| !atom.matches(character))
        })
    }
}

#[derive(Clone, Debug)]
enum ConditionAtom {
    Any,
    Literal(char),
    Class {
        members: BTreeSet<char>,
        negated: bool,
    },
}

impl ConditionAtom {
    fn matches(&self, character: char) -> bool {
        match self {
            Self::Any => true,
            Self::Literal(expected) => *expected == character,
            Self::Class { members, negated } => members.contains(&character) != *negated,
        }
    }
}

fn flag_mode_to_ir(mode: FlagMode) -> FlagModeIr {
    match mode {
        FlagMode::Unicode => FlagModeIr::Unicode,
        FlagMode::Long => FlagModeIr::Long,
        FlagMode::Numeric => FlagModeIr::Numeric,
    }
}

fn case_language_to_ir(language: CaseLanguage) -> CaseLanguageIr {
    match language {
        CaseLanguage::Default => CaseLanguageIr::Default,
        CaseLanguage::Turkic => CaseLanguageIr::Turkic,
    }
}

fn flag_to_ir(flag: Flag, mode: FlagMode) -> FlagIr {
    match mode {
        FlagMode::Numeric => {
            FlagIr::Numeric(u32::try_from(flag.0).expect("validated numeric flags fit in a u32"))
        }
        FlagMode::Unicode | FlagMode::Long => FlagIr::Text(
            decode_text_flag(flag.0).expect("validated text flags contain Unicode scalars"),
        ),
    }
}

fn flags_to_ir(flags: &[Flag], mode: FlagMode) -> BTreeSet<FlagIr> {
    flags
        .iter()
        .copied()
        .map(|flag| flag_to_ir(flag, mode))
        .collect()
}

fn morphology_to_ir(morphology: &Morphology) -> Vec<u32> {
    morphology.iter().map(|id| id.0).collect()
}

fn lexeme_to_ir(lexeme: &Lexeme, flag_mode: FlagMode) -> LexemeIr {
    LexemeIr {
        stem: lexeme.stem.to_string(),
        frequency: None,
        flags: flags_to_ir(&lexeme.flags, flag_mode),
        morphology: morphology_to_ir(&lexeme.morphology),
    }
}

fn affix_rule_to_ir(rule: &AffixRule, flag_mode: FlagMode) -> AffixRuleIr {
    AffixRuleIr {
        id: u32::try_from(rule.id).expect("affix rule IDs are bounded by the importer"),
        kind: match rule.kind {
            AffixKind::Prefix => AffixKindIr::Prefix,
            AffixKind::Suffix => AffixKindIr::Suffix,
        },
        flag: flag_to_ir(rule.flag, flag_mode),
        strip: rule.strip.to_string(),
        add: rule.add.to_string(),
        condition: condition_to_ir(&rule.condition),
        cross_product: rule.cross_product,
        continuation_flags: flags_to_ir(&rule.continuation_flags, flag_mode),
        morphology: morphology_to_ir(&rule.morphology),
    }
}

fn condition_to_ir(condition: &Condition) -> ConditionIr {
    ConditionIr {
        atoms: condition.atoms.iter().map(condition_atom_to_ir).collect(),
        not_preceded_by: condition.not_preceded_by.as_ref().map(condition_atom_to_ir),
        anchored_at_start: condition.anchored_at_start,
    }
}

fn condition_atom_to_ir(atom: &ConditionAtom) -> ConditionAtomIr {
    match atom {
        ConditionAtom::Any => ConditionAtomIr::Any,
        ConditionAtom::Literal(character) => ConditionAtomIr::Literal(*character),
        ConditionAtom::Class { members, negated } => ConditionAtomIr::Class {
            members: members.clone(),
            negated: *negated,
        },
    }
}

fn special_flags_to_ir(flags: &SpecialFlags, flag_mode: FlagMode) -> SpecialFlagsIr {
    SpecialFlagsIr {
        circumfix: flags
            .circumfix
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        forbidden_word: flags
            .forbidden_word
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        keep_case: flags
            .keep_case
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        need_affix: flags
            .need_affix
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        only_in_compound: flags
            .only_in_compound
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        no_suggest: flags
            .no_suggest
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        check_sharps: flags.check_sharps,
    }
}

fn compound_to_ir(compound: &CompoundConfig, flag_mode: FlagMode) -> CompoundConfigIr {
    CompoundConfigIr {
        flag: compound
            .flag
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        begin: compound
            .begin
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        middle: compound
            .middle
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        end: compound
            .end
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        permit: compound
            .permit
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        forbid: compound
            .forbid
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        force_uppercase: compound
            .force_uppercase
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        minimum_length: compound.minimum_length,
        maximum_words: compound.maximum_words,
        check_duplicate: compound.check_duplicate,
        check_replacement: compound.check_replacement,
        check_case: compound.check_case,
        check_triple: compound.check_triple,
        simplified_triple: compound.simplified_triple,
        patterns: compound
            .patterns
            .iter()
            .map(|pattern| compound_pattern_to_ir(pattern, flag_mode))
            .collect(),
        syllable_limit: compound
            .syllable_limit
            .as_ref()
            .map(|limit| CompoundSyllableLimitIr {
                maximum: limit.maximum,
                vowels: limit.vowels.clone(),
            }),
        rules: compound
            .rules
            .iter()
            .map(|rule| {
                rule.patterns
                    .iter()
                    .map(|pattern| {
                        pattern
                            .iter()
                            .map(|flag| flag_to_ir(*flag, flag_mode))
                            .collect()
                    })
                    .collect()
            })
            .collect(),
    }
}

fn compound_pattern_to_ir(pattern: &CompoundPattern, flag_mode: FlagMode) -> CompoundPatternIr {
    CompoundPatternIr {
        ending: pattern.ending.to_string(),
        ending_flag: pattern
            .ending_flag
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        beginning: pattern.beginning.to_string(),
        beginning_flag: pattern
            .beginning_flag
            .as_ref()
            .map(|flag| flag_to_ir(*flag, flag_mode)),
        replacement: pattern.replacement.as_ref().map(ToString::to_string),
    }
}

fn break_pattern_to_ir(pattern: &BreakPattern) -> BreakPatternIr {
    BreakPatternIr {
        text: pattern.text.to_string(),
        at_start: pattern.at_start,
        at_end: pattern.at_end,
    }
}

fn input_conversion_to_ir(conversion: &InputConversion) -> InputConversionIr {
    InputConversionIr {
        from: conversion.from.to_string(),
        to: conversion.to.to_string(),
        at_word_start: conversion.at_word_start,
        at_word_end: conversion.at_word_end,
    }
}

fn replacement_rule_to_ir(rule: &ReplacementRule) -> ReplacementRuleIr {
    ReplacementRuleIr {
        from: rule.from().to_owned(),
        to: rule.to().to_owned(),
        at_word_start: rule.at_word_start(),
        at_word_end: rule.at_word_end(),
    }
}

/// Imports UTF-8 `.aff` and `.dic` text into ferrolex's neutral runtime model.
///
/// The supported feature set is documented in `docs/hunspell-format.md` and
/// `docs/affix-semantics.md`. Unsupported directives remain visible as
/// structured diagnostics instead of receiving guessed semantics.
///
/// # Errors
///
/// In strict mode, returns [`ImportError`] if parsing produced an error
/// diagnostic. Lenient mode always returns the safely understood subset.
pub fn import(
    aff_source: &str,
    aff_text: &str,
    dic_source: &str,
    dic_text: &str,
    mode: ImportMode,
) -> Result<ImportResult, ImportError> {
    import_decoded(aff_source, aff_text, dic_source, dic_text, mode, Vec::new())
}

/// Imports a raw `.aff`/`.dic` pair after discovering its shared byte encoding
/// from the affix file's `SET` declaration.
///
/// `UTF-8`, `ISO-8859-1`, and `ISO-8859-2` declarations are supported. UTF-8
/// decoding rejects malformed byte sequences. The ISO encodings use their
/// defined one-byte mappings and therefore never replace or discard bytes.
///
/// # Errors
///
/// A missing `SET` declaration uses the existing Hunspell-compatible UTF-8
/// default. In strict mode, returns [`ImportError`] if a declared encoding is
/// unsupported, decoding fails, or parsing produces another error diagnostic.
/// Lenient mode retains only the safely decoded subset.
pub fn import_bytes(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    mode: ImportMode,
) -> Result<ImportResult, ImportError> {
    let mut diagnostics = Vec::new();
    if !enforce_byte_input_limits(
        aff_source,
        aff_bytes,
        dic_source,
        dic_bytes,
        &mut diagnostics,
    ) {
        return import_decoded(aff_source, "", dic_source, "", mode, diagnostics);
    }
    let Some(encoding) = declared_encoding(aff_source, aff_bytes, &mut diagnostics) else {
        return import_decoded(aff_source, "", dic_source, "", mode, diagnostics);
    };
    import_bytes_with_declared_encodings(
        aff_source,
        aff_bytes,
        dic_source,
        dic_bytes,
        ByteImportEncodings::same(encoding),
        mode,
        diagnostics,
    )
}

/// Imports a raw `.aff`/`.dic` pair with independently reviewed file encodings.
///
/// The affix file's `SET` declaration must still name the configured affix
/// encoding. The only exception is [`ByteEncoding::Utf8WithIso8859_2Fallback`],
/// which remains compatible with `SET UTF-8` while preserving a reviewed
/// legacy-byte boundary. This prevents an override from silently interpreting
/// a pair with an incompatible declared format. Use this only when a source
/// catalog establishes a dictionary-file exception to the normal shared
/// encoding.
///
/// # Errors
///
/// In strict mode, returns [`ImportError`] if a present declaration is
/// unsupported, disagrees with `encodings.aff()`, decoding fails, or parsing
/// produces another error diagnostic.
pub fn import_bytes_with_encodings(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    encodings: ByteImportEncodings,
    mode: ImportMode,
) -> Result<ImportResult, ImportError> {
    let mut diagnostics = Vec::new();
    if !enforce_byte_input_limits(
        aff_source,
        aff_bytes,
        dic_source,
        dic_bytes,
        &mut diagnostics,
    ) {
        return import_decoded(aff_source, "", dic_source, "", mode, diagnostics);
    }
    let Some(declared) = declared_encoding(aff_source, aff_bytes, &mut diagnostics) else {
        return import_decoded(aff_source, "", dic_source, "", mode, diagnostics);
    };
    if !affix_encoding_matches_set(declared, encodings.aff()) {
        diagnostics.push(diagnostic(
            aff_source,
            1,
            "SET",
            Severity::Error,
            &format!(
                "SET declares {} but the configured affix encoding is {}",
                declared.label(),
                encodings.aff().label()
            ),
        ));
        return import_decoded(aff_source, "", dic_source, "", mode, diagnostics);
    }
    import_bytes_with_declared_encodings(
        aff_source,
        aff_bytes,
        dic_source,
        dic_bytes,
        encodings,
        mode,
        diagnostics,
    )
}

fn affix_encoding_matches_set(declared: ByteEncoding, configured: ByteEncoding) -> bool {
    declared == configured
        || matches!(
            (declared, configured),
            (ByteEncoding::Utf8, ByteEncoding::Utf8WithIso8859_2Fallback)
        )
}

fn import_bytes_with_declared_encodings(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    encodings: ByteImportEncodings,
    mode: ImportMode,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<ImportResult, ImportError> {
    let aff_text = decode_bytes(
        aff_source,
        aff_bytes,
        encodings.aff(),
        true,
        &mut diagnostics,
    );
    let dic_text = decode_bytes(
        dic_source,
        dic_bytes,
        encodings.dic(),
        false,
        &mut diagnostics,
    );
    import_decoded(
        aff_source,
        &aff_text,
        dic_source,
        &dic_text,
        mode,
        diagnostics,
    )
}

fn import_decoded(
    aff_source: &str,
    aff_text: &str,
    dic_source: &str,
    dic_text: &str,
    mode: ImportMode,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<ImportResult, ImportError> {
    let mut parsed_aff =
        if enforce_input_limit(aff_source, aff_text, MAX_AFF_BYTES, &mut diagnostics) {
            parse_aff(aff_source, aff_text)
        } else {
            ParsedAff::default()
        };
    normalize_affix_text_for_ignored_characters(aff_source, &mut parsed_aff, &mut diagnostics);
    diagnostics.extend(parsed_aff.diagnostics.clone());
    let lexemes = if enforce_input_limit(dic_source, dic_text, MAX_DIC_BYTES, &mut diagnostics) {
        parse_dic(
            dic_source,
            dic_text,
            parsed_aff.flag_mode,
            &parsed_aff.flag_aliases,
            &parsed_aff.morphology_aliases,
            &mut parsed_aff.morphology,
            &parsed_aff.ignored_characters,
            &mut diagnostics,
        )
    } else {
        Vec::new()
    };
    let dictionary = HunspellDictionary::from_parts(
        parsed_aff.flag_mode,
        true,
        parsed_aff.case_language,
        parsed_aff.morphology,
        lexemes,
        parsed_aff.prefixes,
        parsed_aff.suffixes,
        parsed_aff.special_flags,
        parsed_aff.compound,
        parsed_aff.break_patterns,
        parsed_aff.word_characters,
        parsed_aff.replacement_rules,
        parsed_aff.keyboard,
        parsed_aff.character_maps,
        parsed_aff.ignored_characters,
        parsed_aff.input_conversions,
        parsed_aff.output_conversions,
        parsed_aff.affix_behavior.full_strip,
        parsed_aff.affix_behavior.complex_prefixes,
    );

    if mode == ImportMode::Strict
        && diagnostics
            .iter()
            .any(|item| item.severity == Severity::Error)
    {
        return Err(ImportError { diagnostics });
    }

    Ok(ImportResult {
        ir: dictionary.to_ir(),
        dictionary,
        diagnostics,
    })
}

fn declared_encoding(
    source: &str,
    bytes: &[u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ByteEncoding> {
    for (index, raw_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let raw_line = if index == 0 {
            raw_line.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw_line)
        } else {
            raw_line
        };
        let line = trim_ascii_whitespace(raw_line);
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let fields = line
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields.first() != Some(&b"SET".as_slice()) {
            continue;
        }
        let line_number = index + 1;
        if fields.len() != 2 {
            diagnostics.push(diagnostic(
                source,
                line_number,
                "SET",
                Severity::Error,
                "SET requires exactly one supported encoding name",
            ));
            return None;
        }
        let Ok(label) = std::str::from_utf8(fields[1]) else {
            diagnostics.push(diagnostic(
                source,
                line_number,
                "SET",
                Severity::Error,
                "SET encoding name must be ASCII",
            ));
            return None;
        };
        if let Some(encoding) = ByteEncoding::from_set_label(label) {
            return Some(encoding);
        }
        diagnostics.push(diagnostic(
            source,
            line_number,
            "SET",
            Severity::Error,
            "SET must name UTF-8, ISO-8859-1, or ISO-8859-2",
        ));
        return None;
    }
    Some(ByteEncoding::Utf8)
}

fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn decode_bytes(
    source: &str,
    bytes: &[u8],
    encoding: ByteEncoding,
    strip_utf8_bom: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let text = match encoding {
        ByteEncoding::Utf8 => match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(error) => {
                diagnostics.push(diagnostic(
                    source,
                    byte_line_number(bytes, error.valid_up_to()),
                    "encoding",
                    Severity::Error,
                    &format!(
                        "UTF-8 decoding failed at byte {} without replacement",
                        error.valid_up_to()
                    ),
                ));
                String::new()
            }
        },
        ByteEncoding::Iso8859_1 => bytes.iter().map(|byte| char::from(*byte)).collect(),
        ByteEncoding::Iso8859_2 => {
            let (text, had_errors) = ISO_8859_2.decode_without_bom_handling(bytes);
            if had_errors {
                diagnostics.push(diagnostic(
                    source,
                    1,
                    "encoding",
                    Severity::Error,
                    "ISO-8859-2 decoding would replace malformed input",
                ));
                String::new()
            } else {
                text.into_owned()
            }
        }
        ByteEncoding::Utf8WithIso8859_2Fallback => decode_utf8_with_iso8859_2_fallback(bytes),
    };
    if strip_utf8_bom {
        text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
    } else {
        text
    }
}

fn decode_utf8_with_iso8859_2_fallback(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    let mut remaining = bytes;

    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                text.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                text.push_str(
                    std::str::from_utf8(&remaining[..valid_up_to])
                        .expect("UTF-8 error valid prefix is valid UTF-8"),
                );
                let invalid_len = error.error_len().unwrap_or(remaining.len() - valid_up_to);
                for byte in &remaining[valid_up_to..valid_up_to + invalid_len] {
                    if *byte == 0x85 {
                        // The reviewed source uses the ISO-8859-2 C1 NEL
                        // byte as a line separator. The parser expects LF.
                        text.push('\n');
                    } else {
                        let encoded = [*byte];
                        let (decoded, had_errors) =
                            ISO_8859_2.decode_without_bom_handling(&encoded);
                        debug_assert!(!had_errors, "one ISO-8859-2 byte always decodes");
                        text.push_str(&decoded);
                    }
                }
                remaining = &remaining[valid_up_to + invalid_len..];
            }
        }
    }

    // Normalize an already-valid UTF-8 NEL too, so both encodings of the
    // source's line separator reach the parser as a normal line boundary.
    text.replace('\u{0085}', "\n")
}

fn byte_line_number(bytes: &[u8], byte_index: usize) -> usize {
    let mut line_number = 1;
    for byte in &bytes[..byte_index] {
        if *byte == b'\n' {
            line_number += 1;
        }
    }
    line_number
}

fn has_triple_at_compound_boundary(word: &str, boundary: usize) -> bool {
    let left = word[..boundary].chars().rev().take(2).collect::<Vec<_>>();
    let right = word[boundary..].chars().take(2).collect::<Vec<_>>();
    let duplicate_before_boundary = matches!(
        (left.as_slice(), right.as_slice()),
        ([last, previous], [next, ..]) if last == previous && last == next
    );
    let duplicate_after_boundary = matches!(
        (left.as_slice(), right.as_slice()),
        ([last, ..], [next, following]) if last == next && last == following
    );
    duplicate_before_boundary || duplicate_after_boundary
}

fn compound_boundaries(word: &str) -> Option<Vec<usize>> {
    let mut boundaries = word
        .char_indices()
        .take(MAX_COMPOUND_SCALARS.saturating_add(1))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if boundaries.len() > MAX_COMPOUND_SCALARS {
        return None;
    }
    boundaries.push(word.len());
    Some(boundaries)
}

#[derive(Default)]
struct ParsedAff {
    flag_mode: FlagMode,
    has_flag_mode: bool,
    case_language: CaseLanguage,
    has_language: bool,
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    diagnostics: Vec<Diagnostic>,
    rule_count: usize,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
    break_patterns: Vec<BreakPattern>,
    word_characters: BTreeSet<char>,
    replacement_rules: Vec<ReplacementRule>,
    keyboard: Option<Box<str>>,
    character_maps: Vec<String>,
    flag_aliases: Vec<Option<FlagSet>>,
    morphology_aliases: Vec<Option<Morphology>>,
    morphology: MorphologyTable,
    ignored_characters: BTreeSet<char>,
    input_conversions: Vec<InputConversion>,
    output_conversions: Vec<InputConversion>,
    affix_behavior: AffixBehavior,
    declared_sections: BTreeSet<CountedSection>,
}

#[derive(Default)]
struct AffixBehavior {
    full_strip: bool,
    complex_prefixes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BreakPattern {
    text: Box<str>,
    at_start: bool,
    at_end: bool,
}

fn default_break_patterns() -> Vec<BreakPattern> {
    vec![
        BreakPattern {
            text: Box::from("-"),
            at_start: false,
            at_end: false,
        },
        BreakPattern {
            text: Box::from("-"),
            at_start: true,
            at_end: false,
        },
        BreakPattern {
            text: Box::from("-"),
            at_start: false,
            at_end: true,
        },
    ]
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum CountedSection {
    ReplacementRules,
    CharacterMaps,
    FlagAliases,
    MorphologyAliases,
    InputConversions,
    OutputConversions,
    BreakPatterns,
    CompoundPatterns,
}

#[allow(
    clippy::too_many_lines,
    reason = "the directive dispatch stays together to preserve the line-oriented parser contract"
)]
fn parse_aff(source: &str, text: &str) -> ParsedAff {
    let mut parsed = ParsedAff {
        break_patterns: default_break_patterns(),
        ..ParsedAff::default()
    };
    let mut lines = text.lines().enumerate();

    while let Some((index, original_line)) = lines.next() {
        let original_line = strip_initial_bom(index, original_line);
        let line = original_line.trim();
        let line_number = index + 1;
        if is_ignored_line(line) {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "line",
                Severity::Error,
                "line exceeds the configured 32 KiB importer limit",
            ));
            continue;
        }

        let fields = aff_fields(line);
        let directive = fields[0];
        match directive {
            "SET" => parse_set(source, line_number, &fields, &mut parsed.diagnostics),
            "FLAG" => parse_flag_mode(source, line_number, &fields, &mut parsed),
            "LANG" => parse_language(source, line_number, &fields, &mut parsed),
            "CIRCUMFIX" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.circumfix,
                &mut parsed.diagnostics,
            ),
            "FORBIDDENWORD" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.forbidden_word,
                &mut parsed.diagnostics,
            ),
            "KEEPCASE" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.keep_case,
                &mut parsed.diagnostics,
            ),
            "NEEDAFFIX" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.need_affix,
                &mut parsed.diagnostics,
            ),
            "CHECKSHARPS" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.check_sharps,
                &mut parsed.diagnostics,
            ),
            "FULLSTRIP" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.affix_behavior.full_strip,
                &mut parsed.diagnostics,
            ),
            "COMPLEXPREFIXES" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.affix_behavior.complex_prefixes,
                &mut parsed.diagnostics,
            ),
            "ONLYINCOMPOUND" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.only_in_compound,
                &mut parsed.diagnostics,
            ),
            "NOSUGGEST" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.special_flags.no_suggest,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDFLAG" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.flag,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDBEGIN" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.begin,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDMIDDLE" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.middle,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDEND" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.end,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDPERMITFLAG" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.permit,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDFORBIDFLAG" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.forbid,
                &mut parsed.diagnostics,
            ),
            "FORCEUCASE" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound.force_uppercase,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDMIN" => parse_compound_minimum(
                source,
                line_number,
                &fields,
                &mut parsed.compound,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDWORDMAX" => parse_compound_word_maximum(
                source,
                line_number,
                &fields,
                parsed.flag_mode,
                &mut parsed.compound,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDSYLLABLE" => parse_compound_syllable_limit(
                source,
                line_number,
                &fields,
                &mut parsed.compound,
                &mut parsed.diagnostics,
            ),
            "CHECKCOMPOUNDDUP" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.check_duplicate,
                &mut parsed.diagnostics,
            ),
            "CHECKCOMPOUNDREP" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.check_replacement,
                &mut parsed.diagnostics,
            ),
            "CHECKCOMPOUNDCASE" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.check_case,
                &mut parsed.diagnostics,
            ),
            "CHECKCOMPOUNDTRIPLE" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.check_triple,
                &mut parsed.diagnostics,
            ),
            "SIMPLIFIEDTRIPLE" => parse_marker(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.simplified_triple,
                &mut parsed.diagnostics,
            ),
            "CHECKCOMPOUNDPATTERN" => {
                parse_compound_patterns(source, &mut lines, line_number, &fields, &mut parsed);
            }
            "COMPOUNDRULE" => {
                parse_compound_rules(source, &mut lines, line_number, &fields, &mut parsed);
            }
            "BREAK" => parse_break_patterns(source, &mut lines, line_number, &fields, &mut parsed),
            "WORDCHARS" => parse_word_characters(
                source,
                line_number,
                &fields,
                &mut parsed.word_characters,
                &mut parsed.diagnostics,
            ),
            "AF" => parse_flag_aliases(source, &mut lines, line_number, &fields, &mut parsed),
            "AM" => parse_morphology_aliases(source, &mut lines, line_number, &fields, &mut parsed),
            "ICONV" => {
                parse_input_conversions(source, &mut lines, line_number, &fields, &mut parsed);
            }
            "OCONV" => {
                parse_output_conversions(source, &mut lines, line_number, &fields, &mut parsed);
            }
            "IGNORE" => parse_ignored_characters(
                source,
                line_number,
                &fields,
                &mut parsed.ignored_characters,
                &mut parsed.diagnostics,
            ),
            "REP" => parse_replacement_rules(source, &mut lines, line_number, &fields, &mut parsed),
            "KEY" => parse_keyboard(source, line_number, &fields, &mut parsed),
            "MAP" => parse_character_maps(source, &mut lines, line_number, &fields, &mut parsed),
            "PFX" | "SFX" => parse_affix_group(
                source,
                directive,
                &mut lines,
                line_number,
                &fields,
                &mut parsed,
            ),
            _ => parse_unknown_directive(source, line_number, directive, &mut parsed.diagnostics),
        }
    }

    parsed
}

fn parse_flag_aliases(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = parse_alias_count(fields) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AF",
            Severity::Error,
            "AF header requires exactly one non-negative alias count",
        ));
        return;
    };
    if count > MAX_AFFIX_ALIASES {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AF",
            Severity::Error,
            "AF alias count exceeds the configured limit of 100,000",
        ));
        return;
    }
    if parsed
        .declared_sections
        .contains(&CountedSection::FlagAliases)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AF",
            Severity::Error,
            "AF may only be declared once",
        ));
        return;
    }
    parsed.declared_sections.insert(CountedSection::FlagAliases);

    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "AF",
                Severity::Error,
                "AF header ended before all declared aliases were supplied",
            ));
            return;
        };
        let alias_fields = aff_fields(line);
        let flags = match alias_fields.as_slice() {
            ["AF"] => Some(Box::default()),
            ["AF", flags]
                if parsed
                    .flag_mode
                    .flag_count(flags)
                    .is_some_and(|count| count <= MAX_FLAGS_PER_ENTRY) =>
            {
                decode_flags(flags, parsed.flag_mode)
            }
            _ => None,
        };
        if flags.is_none() {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "AF",
                Severity::Error,
                "AF aliases require zero or one flag-set field with at most 256 flags",
            ));
        }
        parsed.flag_aliases.push(flags);
    }
}

fn parse_morphology_aliases(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = parse_alias_count(fields) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AM",
            Severity::Warning,
            "AM header requires exactly one non-negative alias count",
        ));
        return;
    };
    if count > MAX_AFFIX_ALIASES {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AM",
            Severity::Warning,
            "AM alias count exceeds the configured limit of 100,000",
        ));
        return;
    }
    if parsed
        .declared_sections
        .contains(&CountedSection::MorphologyAliases)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "AM",
            Severity::Warning,
            "AM may only be declared once",
        ));
        return;
    }
    parsed
        .declared_sections
        .insert(CountedSection::MorphologyAliases);

    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "AM",
                Severity::Warning,
                "AM header ended before all declared aliases were supplied",
            ));
            return;
        };
        let fields = aff_fields(line);
        let alias = fields
            .strip_prefix(&["AM"])
            .filter(|fields| !fields.is_empty())
            .and_then(|fields| {
                intern_morphology_fields(fields, &mut parsed.morphology)
                    .ok()
                    .map(Vec::into_boxed_slice)
            });
        if alias.is_none() {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "AM",
                Severity::Warning,
                "AM aliases require non-empty morphology text",
            ));
        }
        parsed.morphology_aliases.push(alias);
    }
}

fn parse_alias_count(fields: &[&str]) -> Option<usize> {
    (fields.len() == 2)
        .then(|| fields[1].parse().ok())
        .flatten()
}

fn parse_input_conversions(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = parse_alias_count(fields) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "ICONV",
            Severity::Error,
            "ICONV header requires exactly one non-negative rule count",
        ));
        return;
    };
    if count > MAX_INPUT_CONVERSIONS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "ICONV",
            Severity::Error,
            "ICONV rule count exceeds the configured limit of 4096",
        ));
        return;
    }
    if parsed
        .declared_sections
        .contains(&CountedSection::InputConversions)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "ICONV",
            Severity::Error,
            "ICONV may only be declared once",
        ));
        return;
    }
    parsed
        .declared_sections
        .insert(CountedSection::InputConversions);

    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "ICONV",
                Severity::Error,
                "ICONV header ended before all declared rules were supplied",
            ));
            return;
        };
        let rule_fields = aff_fields(line);
        let Some((from, to)) = matches!(rule_fields.as_slice(), ["ICONV", _, _])
            .then(|| (rule_fields[1], rule_fields[2]))
        else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "ICONV",
                Severity::Error,
                "ICONV rules require exactly two non-empty literal strings",
            ));
            continue;
        };
        let (from, at_word_start, at_word_end) = split_conversion_anchors(from);
        let to = if to == "0" { "" } else { to };
        if from.is_empty() || from.len() > MAX_LINE_BYTES || to.len() > MAX_LINE_BYTES {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "ICONV",
                Severity::Error,
                "ICONV rules require a bounded non-empty source string",
            ));
            continue;
        }
        parsed.input_conversions.push(InputConversion {
            from: Box::from(from),
            to: Box::from(to),
            at_word_start,
            at_word_end,
        });
    }
}

fn parse_output_conversions(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = parse_alias_count(fields) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "OCONV",
            Severity::Error,
            "OCONV header requires exactly one non-negative rule count",
        ));
        return;
    };
    if count > MAX_INPUT_CONVERSIONS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "OCONV",
            Severity::Error,
            "OCONV rule count exceeds the configured limit of 4096",
        ));
        return;
    }
    if parsed
        .declared_sections
        .contains(&CountedSection::OutputConversions)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "OCONV",
            Severity::Error,
            "OCONV may only be declared once",
        ));
        return;
    }
    parsed
        .declared_sections
        .insert(CountedSection::OutputConversions);

    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "OCONV",
                Severity::Error,
                "OCONV header ended before all declared rules were supplied",
            ));
            return;
        };
        let rule_fields = aff_fields(line);
        let Some((from, to)) = matches!(rule_fields.as_slice(), ["OCONV", _, _])
            .then(|| (rule_fields[1], rule_fields[2]))
        else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "OCONV",
                Severity::Error,
                "OCONV rules require exactly two non-empty literal strings",
            ));
            continue;
        };
        let (from, at_word_start, at_word_end) = split_conversion_anchors(from);
        let to = if to == "0" { "" } else { to };
        if from.is_empty() || from.len() > MAX_LINE_BYTES || to.len() > MAX_LINE_BYTES {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "OCONV",
                Severity::Error,
                "OCONV rules require a bounded non-empty source string",
            ));
            continue;
        }
        parsed.output_conversions.push(InputConversion {
            from: Box::from(from),
            to: Box::from(to),
            at_word_start,
            at_word_end,
        });
    }
}

fn split_conversion_anchors(value: &str) -> (&str, bool, bool) {
    let at_word_start = value.starts_with('_');
    let value = if at_word_start { &value[1..] } else { value };
    let at_word_end = value.ends_with('_');
    let value = if at_word_end {
        &value[..value.len() - '_'.len_utf8()]
    } else {
        value
    };
    (value, at_word_start, at_word_end)
}

fn apply_conversions(word: &str, conversions: &[InputConversion]) -> String {
    let mut converted = String::with_capacity(word.len());
    let mut index = 0;
    while index < word.len() {
        let remaining = &word[index..];
        let matching = conversions
            .iter()
            .filter(|conversion| {
                (!conversion.at_word_start || index == 0)
                    && (!conversion.at_word_end || conversion.from.len() == remaining.len())
                    && remaining.starts_with(conversion.from.as_ref())
            })
            .fold(None, |best: Option<&InputConversion>, conversion| {
                best.filter(|best| best.from.len() >= conversion.from.len())
                    .or(Some(conversion))
            });
        if let Some(conversion) = matching {
            converted.push_str(&conversion.to);
            index += conversion.from.len();
        } else {
            let character = remaining
                .chars()
                .next()
                .expect("index stays at a UTF-8 character boundary");
            converted.push(character);
            index += character.len_utf8();
        }
    }
    converted
}

fn parse_ignored_characters(
    source: &str,
    line: usize,
    fields: &[&str],
    ignored_characters: &mut BTreeSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fields.len() != 2 || fields[1].is_empty() {
        diagnostics.push(diagnostic(
            source,
            line,
            "IGNORE",
            Severity::Error,
            "IGNORE requires exactly one non-empty Unicode character set",
        ));
        return;
    }
    if !ignored_characters.is_empty() {
        diagnostics.push(diagnostic(
            source,
            line,
            "IGNORE",
            Severity::Error,
            "IGNORE may only be declared once",
        ));
        return;
    }
    ignored_characters.extend(fields[1].chars());
}

fn next_counted_section_line<'source>(
    lines: &mut std::iter::Enumerate<std::str::Lines<'source>>,
) -> Option<(usize, &'source str)> {
    lines.find_map(|(index, line)| (!is_ignored_line(line.trim())).then_some((index, line.trim())))
}

fn normalize_affix_text_for_ignored_characters(
    source: &str,
    parsed: &mut ParsedAff,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if parsed.ignored_characters.is_empty() {
        return;
    }
    for rule in parsed.prefixes.iter_mut().chain(&mut parsed.suffixes) {
        rule.strip = remove_ignored_characters(
            Cow::Borrowed(rule.strip.as_ref()),
            &parsed.ignored_characters,
        );
        rule.add =
            remove_ignored_characters(Cow::Borrowed(rule.add.as_ref()), &parsed.ignored_characters);
        for atom in &rule.condition.atoms {
            if matches!(atom, ConditionAtom::Literal(character) if parsed.ignored_characters.contains(character))
            {
                diagnostics.push(diagnostic(
                    source,
                    1,
                    "IGNORE",
                    Severity::Error,
                    "IGNORE cannot safely remove a literal affix-condition character",
                ));
                break;
            }
            if matches!(atom, ConditionAtom::Class { members, .. } if members.iter().any(|character| parsed.ignored_characters.contains(character)))
            {
                diagnostics.push(diagnostic(
                    source,
                    1,
                    "IGNORE",
                    Severity::Error,
                    "IGNORE cannot safely remove an affix-condition class character",
                ));
                break;
            }
        }
    }
}

fn remove_ignored_characters(value: Cow<'_, str>, ignored_characters: &BTreeSet<char>) -> Box<str> {
    if ignored_characters.is_empty()
        || !value
            .chars()
            .any(|character| ignored_characters.contains(&character))
    {
        return match value {
            Cow::Borrowed(value) => Box::from(value),
            Cow::Owned(value) => value.into_boxed_str(),
        };
    }
    value
        .chars()
        .filter(|character| !ignored_characters.contains(character))
        .collect::<String>()
        .into_boxed_str()
}

fn parse_replacement_rules(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = fields
        .get(1)
        .filter(|_| fields.len() == 2)
        .and_then(|value| value.parse::<usize>().ok())
    else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "REP",
            Severity::Warning,
            "REP header requires exactly one non-negative rule count",
        ));
        return;
    };
    if count > MAX_REPLACEMENT_RULES {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "REP",
            Severity::Warning,
            "REP rule count exceeds the configured limit of 4096",
        ));
        return;
    }
    if parsed
        .declared_sections
        .contains(&CountedSection::ReplacementRules)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "REP",
            Severity::Warning,
            "REP may only be declared once",
        ));
        return;
    }
    parsed
        .declared_sections
        .insert(CountedSection::ReplacementRules);

    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "REP",
                Severity::Warning,
                "REP header ended before all declared rules were supplied",
            ));
            return;
        };
        let rule_fields = aff_fields(line);
        let rule = match rule_fields.as_slice() {
            ["REP", from, to] => parse_replacement_rule(from, to),
            _ => None,
        };
        let Some(rule) = rule else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "REP",
                Severity::Warning,
                "REP rules require exactly two non-empty literal spellings",
            ));
            continue;
        };
        parsed.replacement_rules.push(rule);
    }
}

fn parse_replacement_rule(from: &str, to: &str) -> Option<ReplacementRule> {
    let at_word_start = from.starts_with('^');
    let from = from.strip_prefix('^').unwrap_or(from);
    let at_word_end = from.ends_with('$');
    let from = from.strip_suffix('$').unwrap_or(from);
    ReplacementRule::with_boundaries(from, to, at_word_start, at_word_end)
}

fn parse_keyboard(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
    let Some(layout) = fields.get(1).filter(|_| fields.len() == 2) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "KEY",
            Severity::Warning,
            "KEY requires exactly one non-empty keyboard layout",
        ));
        return;
    };
    if layout.is_empty() || layout.len() > MAX_LINE_BYTES || parsed.keyboard.is_some() {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "KEY",
            Severity::Warning,
            "KEY may only be declared once with a bounded non-empty layout",
        ));
        return;
    }
    parsed.keyboard = Some(Box::from(*layout));
}

fn parse_character_maps(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = parse_alias_count(fields) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "MAP",
            Severity::Warning,
            "MAP header requires exactly one non-negative group count",
        ));
        return;
    };
    if count > MAX_CHARACTER_MAPS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "MAP",
            Severity::Warning,
            "MAP group count exceeds the configured limit of 4096",
        ));
        return;
    }
    if !parsed
        .declared_sections
        .insert(CountedSection::CharacterMaps)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "MAP",
            Severity::Warning,
            "MAP may only be declared once",
        ));
        return;
    }
    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "MAP",
                Severity::Warning,
                "MAP header ended before all declared groups were supplied",
            ));
            return;
        };
        let rule_fields = aff_fields(line);
        let Some(group) = matches!(rule_fields.as_slice(), ["MAP", _]).then_some(rule_fields[1])
        else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "MAP",
                Severity::Warning,
                "MAP groups require exactly one non-empty character group",
            ));
            continue;
        };
        if group.is_empty() || group.len() > MAX_LINE_BYTES || group.chars().count() < 2 {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "MAP",
                Severity::Warning,
                "MAP groups require two or more bounded characters",
            ));
            continue;
        }
        parsed.character_maps.push(group.to_owned());
    }
}

fn parse_unknown_directive(
    source: &str,
    line_number: usize,
    directive: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let suggestion_only = is_suggestion_only_directive(directive);
    diagnostics.push(diagnostic(
        source,
        line_number,
        directive,
        if suggestion_only { Severity::Warning } else { Severity::Error },
        if suggestion_only {
            "suggestion-only directive is not implemented in the current compatibility level"
        } else {
            "directive may affect recognition and is not implemented in the current compatibility level"
        },
    ));
}

fn parse_set(source: &str, line: usize, fields: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    if fields.len() != 2 {
        diagnostics.push(diagnostic(
            source,
            line,
            "SET",
            Severity::Error,
            "SET requires exactly one encoding name",
        ));
    } else if ByteEncoding::from_set_label(fields[1]).is_none() {
        diagnostics.push(diagnostic(
            source,
            line,
            "SET",
            Severity::Error,
            "SET must name UTF-8, ISO-8859-1, or ISO-8859-2",
        ));
    }
}

fn parse_flag_mode(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
    if fields.len() != 2 {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "FLAG",
            Severity::Error,
            "FLAG requires exactly one mode",
        ));
    } else if parsed.has_flag_mode {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "FLAG",
            Severity::Error,
            "FLAG may only be declared once",
        ));
    } else if let Some(flag_mode) = FlagMode::parse(fields[1]) {
        parsed.flag_mode = flag_mode;
        parsed.has_flag_mode = true;
    } else {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "FLAG",
            Severity::Error,
            "FLAG must name UTF-8, UTF8, long, or num",
        ));
    }
}

fn parse_language(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
    if fields.len() != 2 || fields[1].is_empty() {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "LANG",
            Severity::Error,
            "LANG requires exactly one language code",
        ));
    } else if parsed.has_language {
        parsed.diagnostics.push(diagnostic(
            source,
            line,
            "LANG",
            Severity::Error,
            "LANG may only be declared once",
        ));
    } else {
        parsed.case_language = CaseLanguage::from_lang(fields[1]);
        parsed.has_language = true;
    }
}

fn parse_special_flag(
    source: &str,
    line: usize,
    directive: &str,
    fields: &[&str],
    flag_mode: FlagMode,
    target: &mut Option<Flag>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fields.len() != 2 {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive requires exactly one single-Unicode-scalar flag",
        ));
    } else if target.is_some() {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive may only be declared once",
        ));
    } else if let Some(flag) = decode_flag(fields[1], flag_mode) {
        *target = Some(flag);
    } else {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive flag is invalid for the selected FLAG mode",
        ));
    }
}

fn parse_marker(
    source: &str,
    line: usize,
    directive: &str,
    fields: &[&str],
    target: &mut bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fields.len() != 1 {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive does not accept arguments",
        ));
    } else if *target {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive may only be declared once",
        ));
    } else {
        *target = true;
    }
}

fn parse_compound_minimum(
    source: &str,
    line: usize,
    fields: &[&str],
    compound: &mut CompoundConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fields.len() != 2 {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDMIN",
            Severity::Error,
            "COMPOUNDMIN requires exactly one positive scalar length",
        ));
    } else if let Ok(minimum_length) = fields[1].parse::<usize>() {
        compound.minimum_length = minimum_length.max(1);
    } else {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDMIN",
            Severity::Error,
            "COMPOUNDMIN requires a non-negative integer",
        ));
    }
}

fn parse_compound_word_maximum(
    source: &str,
    line: usize,
    fields: &[&str],
    flag_mode: FlagMode,
    compound: &mut CompoundConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(maximum) = fields
        .get(1)
        .filter(|_| {
            fields.len() == 2 || (fields.len() == 3 && decode_flag(fields[2], flag_mode).is_some())
        })
        .and_then(|value| value.parse::<usize>().ok())
    else {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDWORDMAX",
            Severity::Error,
            "COMPOUNDWORDMAX requires a positive component count and an optional legacy flag",
        ));
        return;
    };
    if maximum == 0 || maximum > MAX_COMPOUND_SCALARS {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDWORDMAX",
            Severity::Error,
            "COMPOUNDWORDMAX must be between 1 and 256",
        ));
    } else if compound.maximum_words.is_some() {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDWORDMAX",
            Severity::Error,
            "COMPOUNDWORDMAX may only be declared once",
        ));
    } else {
        compound.maximum_words = Some(maximum);
    }
}

fn parse_compound_syllable_limit(
    source: &str,
    line: usize,
    fields: &[&str],
    compound: &mut CompoundConfig,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(maximum) = fields
        .get(1)
        .filter(|_| fields.len() == 3)
        .and_then(|value| value.parse::<usize>().ok())
    else {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDSYLLABLE",
            Severity::Error,
            "COMPOUNDSYLLABLE requires a non-negative limit and a non-empty vowel set",
        ));
        return;
    };
    if fields[2].is_empty() || compound.syllable_limit.is_some() {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDSYLLABLE",
            Severity::Error,
            "COMPOUNDSYLLABLE may only be declared once with a non-empty vowel set",
        ));
        return;
    }
    compound.syllable_limit = Some(CompoundSyllableLimit {
        maximum,
        vowels: fields[2].chars().collect(),
    });
}

fn parse_compound_patterns(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Some(count) = fields
        .get(1)
        .filter(|_| fields.len() == 2)
        .and_then(|value| value.parse::<usize>().ok())
    else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "CHECKCOMPOUNDPATTERN",
            Severity::Error,
            "CHECKCOMPOUNDPATTERN requires exactly one positive rule count",
        ));
        return;
    };
    if count == 0 || count > MAX_COMPOUND_PATTERNS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "CHECKCOMPOUNDPATTERN",
            Severity::Error,
            "CHECKCOMPOUNDPATTERN count must be between 1 and 1024",
        ));
        return;
    }
    if !parsed
        .declared_sections
        .insert(CountedSection::CompoundPatterns)
    {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "CHECKCOMPOUNDPATTERN",
            Severity::Error,
            "CHECKCOMPOUNDPATTERN may only be declared once",
        ));
        return;
    }
    for _ in 0..count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "CHECKCOMPOUNDPATTERN",
                Severity::Error,
                "CHECKCOMPOUNDPATTERN header ended before all declared rules were supplied",
            ));
            return;
        };
        let fields = aff_fields(line);
        let pattern = match fields.as_slice() {
            ["CHECKCOMPOUNDPATTERN", ending, beginning] => {
                parse_compound_pattern(ending, beginning, None, parsed.flag_mode)
            }
            ["CHECKCOMPOUNDPATTERN", ending, beginning, replacement] => {
                parse_compound_pattern(ending, beginning, Some(replacement), parsed.flag_mode)
            }
            _ => None,
        };
        let Some(pattern) = pattern else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "CHECKCOMPOUNDPATTERN",
                Severity::Error,
                "compound patterns require endchars[/flag], beginchars[/flag], and an optional replacement",
            ));
            continue;
        };
        parsed.compound.patterns.push(pattern);
    }
}

fn parse_compound_pattern(
    ending: &str,
    beginning: &str,
    replacement: Option<&str>,
    flag_mode: FlagMode,
) -> Option<CompoundPattern> {
    let (ending, ending_flag) = parse_compound_pattern_part(ending, flag_mode)?;
    let (beginning, beginning_flag) = parse_compound_pattern_part(beginning, flag_mode)?;
    if ending.is_empty()
        && beginning.is_empty()
        && ending_flag.is_none()
        && beginning_flag.is_none()
    {
        return None;
    }
    let replacement = replacement
        .filter(|replacement| !replacement.is_empty())
        .map(Box::<str>::from);
    Some(CompoundPattern {
        ending,
        ending_flag,
        beginning,
        beginning_flag,
        replacement,
    })
}

fn parse_compound_pattern_part(
    value: &str,
    flag_mode: FlagMode,
) -> Option<(Box<str>, Option<Flag>)> {
    let (text, flag) = match value.split_once('/') {
        Some((text, flag)) => (text, Some(decode_flag(flag, flag_mode)?)),
        None => (value, None),
    };
    (!text.contains('/')).then(|| (text.into(), flag))
}

fn parse_compound_rules(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Ok(rule_count) = fields.get(1).unwrap_or(&"").parse::<usize>() else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "COMPOUNDRULE",
            Severity::Error,
            "COMPOUNDRULE header requires a positive rule count",
        ));
        return;
    };
    if fields.len() != 2 || rule_count == 0 || rule_count > MAX_COMPOUND_RULES {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "COMPOUNDRULE",
            Severity::Error,
            "COMPOUNDRULE count must be between 1 and 1024",
        ));
        return;
    }
    let mut expansion_count = 0_usize;
    for _ in 0..rule_count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "COMPOUNDRULE",
                Severity::Error,
                "COMPOUNDRULE header ended before all declared rules were supplied",
            ));
            return;
        };
        let line = line.trim();
        let rule_fields = aff_fields(line);
        let pattern = rule_fields.get(1).copied().unwrap_or_default();
        let patterns = parse_compound_rule_patterns(pattern, parsed.flag_mode);
        if rule_fields.len() != 2 || rule_fields[0] != "COMPOUNDRULE" || patterns.is_err() {
            let message = patterns.err().unwrap_or(
                "compound rules require bounded literal flags with optional postfix `*`, `+`, or `?`",
            );
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "COMPOUNDRULE",
                Severity::Error,
                message,
            ));
            continue;
        }
        let patterns = patterns.expect("validated above");
        if expansion_count.saturating_add(patterns.len()) > MAX_COMPOUND_RULE_EXPANSIONS {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "COMPOUNDRULE",
                Severity::Error,
                "compound rule expansions exceed the dictionary limit of 16,384",
            ));
            continue;
        }
        expansion_count += patterns.len();
        parsed.compound.rules.push(CompoundRule { patterns });
    }
}

fn parse_compound_rule_patterns(
    pattern: &str,
    flag_mode: FlagMode,
) -> Result<Vec<Vec<Flag>>, &'static str> {
    if pattern.contains(['(', ')']) {
        return parse_parenthesized_compound_rule(pattern, flag_mode).map(|pattern| vec![pattern]);
    }
    if flag_mode != FlagMode::Unicode {
        return decode_flag_sequence(pattern, flag_mode)
            .filter(|flags| (2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&flags.len()))
            .map(|flags| vec![flags])
            .ok_or("compound rules require two through sixteen literal flags");
    }
    let tokens =
        unicode_flag_tokens(pattern).ok_or("compound rules require valid Unicode flag tokens")?;
    let mut parts = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if matches!(token, "*" | "+" | "?") {
            return Err("compound quantifiers must follow a flag");
        }
        index += 1;
        let (minimum, maximum) = match tokens.get(index) {
            Some(&"*") => {
                index += 1;
                (0, MAX_COMPOUND_RULE_COMPONENTS)
            }
            Some(&"+") => {
                index += 1;
                (1, MAX_COMPOUND_RULE_COMPONENTS)
            }
            Some(&"?") => {
                index += 1;
                (0, 1)
            }
            _ => (1, 1),
        };
        parts.push((
            decode_flag(token, FlagMode::Unicode).ok_or("compound rule flag is invalid")?,
            minimum,
            maximum,
        ));
    }
    let mut patterns = vec![Vec::new()];
    for (flag, minimum, maximum) in parts {
        let mut expanded = Vec::new();
        for prefix in patterns {
            for count in minimum..=maximum.min(MAX_COMPOUND_RULE_COMPONENTS - prefix.len()) {
                let mut next = prefix.clone();
                next.extend(std::iter::repeat_n(flag, count));
                expanded.push(next);
                if expanded.len() > MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE {
                    return Err("compound rule expansions exceed the per-rule limit of 1,024");
                }
            }
        }
        patterns = expanded;
    }
    patterns.retain(|flags| (2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&flags.len()));
    (!patterns.is_empty())
        .then_some(patterns)
        .ok_or("compound rules require two through sixteen components")
}

fn parse_parenthesized_compound_rule(
    pattern: &str,
    flag_mode: FlagMode,
) -> Result<Vec<Flag>, &'static str> {
    let mut flags = Vec::new();
    let mut remaining = pattern;
    while let Some(group) = remaining.strip_prefix('(') {
        let Some((flag, rest)) = group.split_once(')') else {
            return Err("parenthesized COMPOUNDRULE groups must be balanced");
        };
        if flag.is_empty() || flag.contains(['(', ')', '*', '+', '?']) {
            return Err("parenthesized COMPOUNDRULE groups require one literal flag");
        }
        let mut decoded = decode_flag_sequence(flag, flag_mode)
            .ok_or("parenthesized COMPOUNDRULE groups require one valid flag")?;
        if decoded.len() != 1 {
            return Err("parenthesized COMPOUNDRULE groups require one literal flag");
        }
        flags.push(decoded.pop().expect("one checked flag"));
        remaining = rest;
    }
    if !remaining.is_empty() || !(2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&flags.len()) {
        return Err("parenthesized COMPOUNDRULE groups require two through sixteen literal flags");
    }
    Ok(flags)
}

fn parse_break_patterns(
    source: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    let Ok(pattern_count) = fields.get(1).unwrap_or(&"").parse::<usize>() else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "BREAK",
            Severity::Error,
            "BREAK header requires a positive pattern count",
        ));
        return;
    };
    if fields.len() != 2 || pattern_count > MAX_BREAK_PATTERNS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "BREAK",
            Severity::Error,
            "BREAK count must be between 0 and 256",
        ));
        return;
    }
    if !parsed
        .declared_sections
        .contains(&CountedSection::BreakPatterns)
    {
        parsed.break_patterns.clear();
        parsed
            .declared_sections
            .insert(CountedSection::BreakPatterns);
    }
    for _ in 0..pattern_count {
        let Some((index, line)) = next_counted_section_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "BREAK",
                Severity::Error,
                "BREAK header ended before all declared patterns were supplied",
            ));
            return;
        };
        let rule_fields = aff_fields(line);
        let pattern = rule_fields.get(1).copied().unwrap_or_default();
        let Some(pattern) = parse_break_pattern(pattern) else {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "BREAK",
                Severity::Error,
                "BREAK requires a non-empty literal pattern with an optional start or end anchor",
            ));
            continue;
        };
        if rule_fields.len() != 2 || rule_fields[0] != "BREAK" {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "BREAK",
                Severity::Error,
                "BREAK rules require exactly one pattern",
            ));
            continue;
        }
        parsed.break_patterns.push(pattern);
    }
}

fn parse_break_pattern(value: &str) -> Option<BreakPattern> {
    let at_start = value.starts_with('^');
    let at_end = value.ends_with('$');
    if at_start && at_end {
        return None;
    }
    let value = value.strip_prefix('^').unwrap_or(value);
    let value = value.strip_suffix('$').unwrap_or(value);
    (!value.is_empty() && !value.contains(['^', '$'])).then(|| BreakPattern {
        text: Box::from(value),
        at_start,
        at_end,
    })
}

fn parse_word_characters(
    source: &str,
    line: usize,
    fields: &[&str],
    word_characters: &mut BTreeSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if fields.len() != 2 || fields[1].is_empty() {
        diagnostics.push(diagnostic(
            source,
            line,
            "WORDCHARS",
            Severity::Error,
            "WORDCHARS requires exactly one non-empty Unicode character set",
        ));
        return;
    }
    if !word_characters.is_empty() {
        diagnostics.push(diagnostic(
            source,
            line,
            "WORDCHARS",
            Severity::Error,
            "WORDCHARS may only be declared once",
        ));
        return;
    }
    word_characters.extend(fields[1].chars());
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_affix_group(
    source: &str,
    directive: &str,
    lines: &mut std::iter::Enumerate<std::str::Lines<'_>>,
    line_number: usize,
    fields: &[&str],
    parsed: &mut ParsedAff,
) {
    if fields.len() != 4 {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            directive,
            Severity::Error,
            "affix header must declare a flag, cross-product marker, and rule count",
        ));
        return;
    }
    let Some(flag) = decode_flag(fields[1], parsed.flag_mode) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            directive,
            Severity::Error,
            "affix flag is invalid for the selected FLAG mode",
        ));
        return;
    };
    let cross_product = match fields[2] {
        "Y" => true,
        "N" => false,
        _ => {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                directive,
                Severity::Error,
                "cross-product marker must be `Y` or `N`",
            ));
            return;
        }
    };
    let Ok(rule_count) = fields[3].parse::<usize>() else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            directive,
            Severity::Error,
            "rule count must be a non-negative integer",
        ));
        return;
    };

    let kind = if directive == "PFX" {
        AffixKind::Prefix
    } else {
        AffixKind::Suffix
    };
    let mut consumed_rules = 0;
    while consumed_rules < rule_count {
        let Some((index, original_line)) = lines.next() else {
            break;
        };
        let rule_line = original_line.trim();
        let rule_line_number = index + 1;
        if is_ignored_line(rule_line) {
            continue;
        }
        if rule_line.len() > MAX_LINE_BYTES {
            parsed.diagnostics.push(diagnostic(
                source,
                rule_line_number,
                directive,
                Severity::Error,
                "line exceeds the configured 32 KiB importer limit",
            ));
            consumed_rules += 1;
            continue;
        }
        consumed_rules += 1;
        if parsed.rule_count == MAX_AFFIX_RULES {
            parsed.diagnostics.push(diagnostic(
                source,
                rule_line_number,
                directive,
                Severity::Error,
                "affix rule limit of 100,000 has been exceeded",
            ));
            continue;
        }
        match parse_affix_rule(
            parsed.rule_count,
            directive,
            flag,
            cross_product,
            parsed.flag_mode,
            &parsed.flag_aliases,
            rule_line,
            &mut parsed.morphology,
        ) {
            Ok(rule) => {
                match kind {
                    AffixKind::Prefix => parsed.prefixes.push(rule),
                    AffixKind::Suffix => parsed.suffixes.push(rule),
                }
                parsed.rule_count += 1;
            }
            Err(message) => parsed.diagnostics.push(diagnostic(
                source,
                rule_line_number,
                directive,
                Severity::Error,
                &message,
            )),
        }
    }
    if consumed_rules != rule_count {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            directive,
            Severity::Error,
            "affix header ended before all declared rules were supplied",
        ));
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parsed affix header and its bounded metadata are validated together"
)]
fn parse_affix_rule(
    id: usize,
    expected_directive: &str,
    header_flag: Flag,
    cross_product: bool,
    flag_mode: FlagMode,
    flag_aliases: &[Option<FlagSet>],
    line: &str,
    morphology_table: &mut MorphologyTable,
) -> Result<AffixRule, String> {
    let fields = aff_fields(line);
    if fields.len() < 4 {
        return Err("affix rule requires a directive, flag, strip, and add".to_owned());
    }
    if fields[0] != expected_directive {
        return Err("affix rule does not match its header directive".to_owned());
    }
    let Some(rule_flag) = decode_flag(fields[1], flag_mode) else {
        return Err("affix rule flag is invalid for the selected FLAG mode".to_owned());
    };
    if rule_flag != header_flag {
        return Err("affix rule flag does not match its header".to_owned());
    }
    let (add, continuation_flags) = match fields[3].split_once('/') {
        None => (fields[3], Box::default()),
        Some((_, "")) => return Err("affix continuation flags must not be empty".to_owned()),
        Some((_, flags))
            if !is_flag_alias_reference(flags, flag_aliases)
                && flag_mode
                    .flag_count(flags)
                    .is_none_or(|count| count > MAX_FLAGS_PER_ENTRY) =>
        {
            return Err("affix continuation flags exceed the 4096-flag importer limit".to_owned())
        }
        Some((add, flags)) => decode_entry_flags(flags, flag_mode, flag_aliases)
            .map(|flags| (add, flags))
            .ok_or_else(|| "affix continuation flags are invalid".to_owned())?,
    };
    let condition = parse_condition(fields.get(4).copied().unwrap_or("."))?;
    let morphology =
        intern_morphology_fields(fields.get(5..).unwrap_or_default(), morphology_table)
            .map_err(str::to_owned)?
            .into_boxed_slice();
    Ok(AffixRule {
        id,
        kind: if expected_directive == "PFX" {
            AffixKind::Prefix
        } else {
            AffixKind::Suffix
        },
        flag: rule_flag,
        strip: empty_marker(fields[2]),
        add: empty_marker(add),
        condition,
        cross_product,
        continuation_flags,
        morphology,
    })
}

fn parse_condition(field: &str) -> Result<Condition, String> {
    if field == "0" {
        return Ok(Condition::empty());
    }

    let (not_preceded_by, field) = parse_negative_lookbehind(field)?;
    let (anchored_at_start, field) = parse_start_anchor(field)?;
    let atoms = parse_condition_atoms(field)?;
    Ok(Condition {
        atoms,
        not_preceded_by,
        anchored_at_start,
    })
}

fn parse_start_anchor(field: &str) -> Result<(bool, &str), String> {
    let Some(rest) = field.strip_prefix("(^") else {
        return Ok((false, field));
    };
    let Some((anchored, trailing)) = rest.split_once(')') else {
        return Err("condition has an unterminated start anchor".to_owned());
    };
    if anchored.is_empty() {
        return Err("condition start anchor must contain a literal pattern".to_owned());
    }
    if !trailing.is_empty() {
        return Err("condition start anchor must end the pattern".to_owned());
    }
    Ok((true, anchored))
}

fn parse_negative_lookbehind(field: &str) -> Result<(Option<ConditionAtom>, &str), String> {
    if let Some(rest) = field.strip_prefix("(?<!") {
        let Some((lookbehind, rest)) = rest.split_once(')') else {
            return Err("condition has an unterminated negative lookbehind".to_owned());
        };
        return Ok((Some(parse_condition_atom(lookbehind)?), rest));
    }
    if let Some(rest) = field.strip_prefix("(^|") {
        let Some((alternative, rest)) = rest.split_once(')') else {
            return Err("condition has an unterminated start-or-class alternative".to_owned());
        };
        let ConditionAtom::Class { members, negated } = parse_condition_atom(alternative)? else {
            return Err("condition start alternative must contain a bracket class".to_owned());
        };
        if !negated {
            return Err(
                "condition start alternative must contain a negated bracket class".to_owned(),
            );
        }
        return Ok((
            Some(ConditionAtom::Class {
                members,
                negated: false,
            }),
            rest,
        ));
    }
    Ok((None, field))
}

fn parse_condition_atoms(field: &str) -> Result<Vec<ConditionAtom>, String> {
    let characters = field.chars().collect::<Vec<_>>();
    if characters.len() > MAX_CONDITION_ATOMS {
        return Err("condition exceeds the configured 256-atom importer limit".to_owned());
    }
    let mut atoms = Vec::new();
    let mut index = 0;
    while let Some(character) = characters.get(index).copied() {
        match character {
            '.' => {
                atoms.push(ConditionAtom::Any);
                index += 1;
            }
            '[' => {
                let Some(end_offset) = characters[index + 1..]
                    .iter()
                    .position(|character| *character == ']')
                else {
                    return Err("condition has an unterminated bracket class".to_owned());
                };
                let end = index + 1 + end_offset;
                atoms.push(parse_condition_atom(
                    &characters[index..=end].iter().collect::<String>(),
                )?);
                index = end + 1;
            }
            ']' | '(' | ')' | '|' | '*' | '?' | '\\' => {
                return Err("condition uses syntax outside the supported subset".to_owned())
            }
            literal => {
                atoms.push(ConditionAtom::Literal(literal));
                index += 1;
            }
        }
    }
    Ok(atoms)
}

fn parse_condition_atom(field: &str) -> Result<ConditionAtom, String> {
    if field == "." {
        return Ok(ConditionAtom::Any);
    }
    let characters = field.chars().collect::<Vec<_>>();
    if characters.len() == 1 {
        return Ok(ConditionAtom::Literal(characters[0]));
    }
    if characters.first() != Some(&'[') || characters.last() != Some(&']') {
        return Err("condition lookbehind must contain one literal or bracket class".to_owned());
    }
    let (negated, member_start) = if characters.get(1) == Some(&'^') {
        (true, 2)
    } else {
        (false, 1)
    };
    let member_end = characters.len() - 1;
    if member_start == member_end {
        return Err("condition has an empty bracket class".to_owned());
    }
    Ok(ConditionAtom::Class {
        members: characters[member_start..member_end]
            .iter()
            .copied()
            .collect(),
        negated,
    })
}

fn empty_marker(value: &str) -> Box<str> {
    Box::<str>::from(if value == "0" { "" } else { value })
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "dictionary entry fields and their source-aware diagnostics are parsed together"
)]
fn parse_dic(
    source: &str,
    text: &str,
    flag_mode: FlagMode,
    flag_aliases: &[Option<FlagSet>],
    morphology_aliases: &[Option<Morphology>],
    morphology_table: &mut MorphologyTable,
    ignored_characters: &BTreeSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Lexeme> {
    let mut entries = Vec::new();
    let mut expected_count = None;
    let mut first_content = true;
    let mut entry_count = 0;

    for (index, original_line) in text.lines().enumerate() {
        let original_line = strip_initial_bom(index, original_line);
        let line = original_line.trim();
        if is_ignored_dictionary_line(line) {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            diagnostics.push(diagnostic(
                source,
                index + 1,
                "entry",
                Severity::Error,
                "line exceeds the configured 32 KiB importer limit",
            ));
            continue;
        }
        if first_content {
            first_content = false;
            if let Ok(count) = line.parse::<usize>() {
                expected_count = Some((index + 1, count));
                continue;
            }
        }
        entry_count += 1;
        if entry_count > MAX_DICTIONARY_ENTRIES {
            diagnostics.push(diagnostic(
                source,
                index + 1,
                "entry",
                Severity::Error,
                "dictionary entry limit of 1,000,000 has been exceeded",
            ));
            break;
        }
        let mut fields = line.split_whitespace();
        let field = fields.next().unwrap_or_default();
        let (stem, flags) = split_dictionary_entry(field);
        let stem = remove_ignored_characters(stem, ignored_characters);
        if stem.is_empty() {
            diagnostics.push(diagnostic(
                source,
                index + 1,
                "entry",
                Severity::Error,
                "dictionary entry has no stem",
            ));
            continue;
        }
        let entry_flags = match flags {
            None => Box::default(),
            // A handful of real-world dictionaries use `word/ morphology` to
            // attach morphology without assigning flags. Retain that metadata
            // while continuing to reject a bare trailing delimiter.
            Some("") if fields.clone().next().is_some() => Box::default(),
            Some("") => {
                diagnostics.push(diagnostic(
                    source,
                    index + 1,
                    "entry",
                    Severity::Error,
                    "dictionary entry has an empty flag section",
                ));
                continue;
            }
            Some(value) => {
                if let Some((flags, flag_count)) =
                    decode_entry_flags_with_count(value, flag_mode, flag_aliases)
                {
                    if flag_count <= MAX_FLAGS_PER_ENTRY {
                        flags
                    } else {
                        diagnostics.push(diagnostic(
                            source,
                            index + 1,
                            "entry",
                            Severity::Error,
                            "dictionary entry exceeds the 4096-flag importer limit",
                        ));
                        continue;
                    }
                } else {
                    diagnostics.push(diagnostic(
                        source,
                        index + 1,
                        "entry",
                        Severity::Error,
                        "dictionary entry has an invalid flag section",
                    ));
                    continue;
                }
            }
        };
        let morphology = decode_entry_morphology(
            source,
            index + 1,
            fields,
            morphology_aliases,
            morphology_table,
            diagnostics,
        );
        entries.push(Lexeme {
            stem,
            flags: entry_flags,
            morphology,
        });
    }

    if let Some((count_line, expected_count)) =
        expected_count.filter(|(_, expected)| *expected != entry_count)
    {
        diagnostics.push(diagnostic(
            source,
            count_line,
            "count",
            Severity::Warning,
            &format!("declared {expected_count} entries but parsed {entry_count}"),
        ));
    }
    entries.sort_by(|left, right| left.stem.cmp(&right.stem));
    entries
}

fn strip_initial_bom(line_index: usize, line: &str) -> &str {
    if line_index == 0 {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    }
}

fn decode_entry_flags(
    value: &str,
    flag_mode: FlagMode,
    aliases: &[Option<FlagSet>],
) -> Option<FlagSet> {
    decode_entry_flags_with_count(value, flag_mode, aliases).map(|(flags, _)| flags)
}

fn decode_entry_flags_with_count(
    value: &str,
    flag_mode: FlagMode,
    aliases: &[Option<FlagSet>],
) -> Option<(FlagSet, usize)> {
    if is_flag_alias_reference(value, aliases) {
        let alias = value.parse::<usize>().ok()?.checked_sub(1)?;
        let flags = aliases.get(alias)?.clone()?;
        let count = flags.len();
        Some((flags, count))
    } else {
        let flags = decode_flag_sequence(value, flag_mode)?;
        let count = flags.len();
        Some((flag_set(flags), count))
    }
}

fn is_flag_alias_reference(value: &str, aliases: &[Option<FlagSet>]) -> bool {
    !aliases.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn decode_entry_morphology<'field>(
    source: &str,
    line: usize,
    fields: impl Iterator<Item = &'field str> + Clone,
    aliases: &[Option<Morphology>],
    table: &mut MorphologyTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Morphology {
    let mut fields = fields;
    let Some(first) = fields.next() else {
        return Box::default();
    };
    let mut morphology = if !aliases.is_empty() && first.bytes().all(|byte| byte.is_ascii_digit()) {
        let alias = first
            .parse::<usize>()
            .ok()
            .and_then(|value| value.checked_sub(1));
        if let Some(fields) = alias
            .and_then(|index| aliases.get(index))
            .and_then(Option::as_ref)
        {
            fields.to_vec()
        } else {
            diagnostics.push(diagnostic(
                source,
                line,
                "AM",
                Severity::Warning,
                "dictionary entry references an undefined AM morphology alias",
            ));
            Vec::new()
        }
    } else {
        intern_morphology_field_iter(std::iter::once(first), table).unwrap_or_else(|message| {
            diagnostics.push(diagnostic(source, line, "entry", Severity::Error, message));
            Vec::new()
        })
    };
    match intern_morphology_field_iter(fields, table) {
        Ok(fields) => morphology.extend(fields),
        Err(message) => {
            diagnostics.push(diagnostic(source, line, "entry", Severity::Error, message));
        }
    }
    morphology.into_boxed_slice()
}

fn intern_morphology_fields(
    fields: &[&str],
    table: &mut MorphologyTable,
) -> Result<Vec<MorphologyId>, &'static str> {
    intern_morphology_field_iter(fields.iter().copied(), table)
}

fn intern_morphology_field_iter<'field>(
    fields: impl Iterator<Item = &'field str> + Clone,
    table: &mut MorphologyTable,
) -> Result<Vec<MorphologyId>, &'static str> {
    if fields.clone().count() > MAX_MORPHOLOGY_FIELDS_PER_RECORD {
        return Err("morphology fields exceed the 256-field importer limit");
    }
    fields
        .map(|field| {
            table
                .intern(field)
                .ok_or("morphology string count exceeds the 1,000,000 importer limit")
        })
        .collect()
}

fn decode_flags(value: &str, flag_mode: FlagMode) -> Option<FlagSet> {
    decode_flag_sequence(value, flag_mode).map(flag_set)
}

fn decode_flag_sequence(value: &str, flag_mode: FlagMode) -> Option<Vec<Flag>> {
    if flag_mode == FlagMode::Numeric {
        return value
            .split(',')
            .map(|flag| flag.parse::<u32>().ok().map(|flag| Flag(u64::from(flag))))
            .collect();
    }
    if flag_mode == FlagMode::Unicode {
        return unicode_flag_tokens(value).map(|tokens| {
            tokens
                .into_iter()
                .map(|token| Flag(encode_text_flag(token).expect("one Unicode flag")))
                .collect()
        });
    }
    let characters = value.chars().collect::<Vec<_>>();
    (!characters.is_empty() && characters.len() % 2 == 0).then(|| {
        characters
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| {
                let first = u64::from(u32::from(chunk[0]));
                let second = u64::from(u32::from(chunk[1])) + 1;
                Flag((first << 32) | second)
            })
            .collect()
    })
}

fn flag_set(flags: impl IntoIterator<Item = Flag>) -> FlagSet {
    let mut flags = flags.into_iter().collect::<Vec<_>>();
    flags.sort_unstable();
    flags.dedup();
    flags.into_boxed_slice()
}

fn unicode_flag_tokens(value: &str) -> Option<Vec<&str>> {
    let characters = value.char_indices().collect::<Vec<_>>();
    (!characters.is_empty()).then_some(())?;

    let mut tokens = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let (start, character) = characters[index];
        if is_variation_selector(character) {
            return None;
        }
        index += 1;
        if characters
            .get(index)
            .is_some_and(|(_, character)| is_variation_selector(*character))
        {
            index += 1;
        }
        let end = characters
            .get(index)
            .map_or(value.len(), |(offset, _)| *offset);
        tokens.push(&value[start..end]);
    }
    Some(tokens)
}

const fn is_variation_selector(character: char) -> bool {
    matches!(character, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

fn decode_flag(value: &str, flag_mode: FlagMode) -> Option<Flag> {
    let mut flags = decode_flag_sequence(value, flag_mode)?;
    (flags.len() == 1).then(|| flags.pop().expect("one flag"))
}

impl FlagMode {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "UTF-8" | "UTF8" => Some(Self::Unicode),
            "LONG" => Some(Self::Long),
            "NUM" => Some(Self::Numeric),
            _ => None,
        }
    }

    fn flag_count(self, value: &str) -> Option<usize> {
        if self == Self::Numeric {
            return (!value.is_empty() && value.split(',').all(|flag| flag.parse::<u32>().is_ok()))
                .then(|| value.split(',').count());
        }
        if self == Self::Unicode {
            return unicode_flag_tokens(value).map(|tokens| tokens.len());
        }
        let count = value.chars().count();
        count.is_multiple_of(2).then_some(count / 2)
    }
}

fn aff_fields(line: &str) -> Vec<&str> {
    line.split_whitespace()
        .take_while(|field| !field.starts_with('#'))
        .collect()
}

fn is_ignored_line(line: &str) -> bool {
    line.is_empty() || line.starts_with('#')
}

fn is_ignored_dictionary_line(line: &str) -> bool {
    is_ignored_line(line) || line.starts_with('/')
}

fn split_dictionary_entry(field: &str) -> (Cow<'_, str>, Option<&str>) {
    let mut escaped = false;
    for (index, character) in field.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '/' {
            return (
                unescape_dictionary_stem(&field[..index]),
                Some(&field[index + 1..]),
            );
        }
    }
    (unescape_dictionary_stem(field), None)
}

fn unescape_dictionary_stem(value: &str) -> Cow<'_, str> {
    if !value
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'\\' && matches!(pair[1], b'/' | b'\\'))
    {
        return Cow::Borrowed(value);
    }
    let mut stem = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' && matches!(characters.clone().next(), Some('/' | '\\')) {
            stem.push(characters.next().expect("checked escaped character"));
        } else {
            stem.push(character);
        }
    }
    Cow::Owned(stem)
}

fn enforce_input_limit(
    source: &str,
    text: &str,
    limit: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if text.len() <= limit {
        return true;
    }
    diagnostics.push(diagnostic(
        source,
        1,
        "input",
        Severity::Error,
        &format!(
            "input exceeds the configured {} MiB importer limit",
            limit / (1024 * 1024)
        ),
    ));
    false
}

fn enforce_byte_input_limits(
    aff_source: &str,
    aff_bytes: &[u8],
    dic_source: &str,
    dic_bytes: &[u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let aff_in_limit =
        enforce_byte_input_limit(aff_source, aff_bytes.len(), MAX_AFF_BYTES, diagnostics);
    let dic_in_limit =
        enforce_byte_input_limit(dic_source, dic_bytes.len(), MAX_DIC_BYTES, diagnostics);
    aff_in_limit && dic_in_limit
}

fn enforce_byte_input_limit(
    source: &str,
    byte_length: usize,
    limit: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if byte_length <= limit {
        return true;
    }
    diagnostics.push(diagnostic(
        source,
        1,
        "input",
        Severity::Error,
        &format!(
            "input exceeds the configured {} MiB importer limit",
            limit / (1024 * 1024)
        ),
    ));
    false
}

fn is_suggestion_only_directive(directive: &str) -> bool {
    matches!(
        directive,
        "MAXCPDSUGS"
            | "MAXDIFF"
            | "MAXNGRAMSUGS"
            | "NGRAMSUGS"
            | "NOSPLITSUGS"
            | "ONLYMAXDIFF"
            | "PHONE"
            | "SUGSWITHDOTS"
            | "TRY"
            | "WARN"
            | "FORBIDWARN"
            | "HOME"
            | "NAME"
            | "VERSION"
    )
}

fn diagnostic(
    source: &str,
    line: usize,
    directive: &str,
    severity: Severity,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        source: source.to_owned(),
        line,
        directive: directive.to_owned(),
        severity,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt::Write as _;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Arc;
    use std::thread;

    use ferrolex_core::Dictionary;
    use ferrolex_suggest::{CandidateSource, Completeness, SuggestConfig, Suggester};

    use super::{
        compile_runtime_cache, import, import_bytes, import_bytes_with_encodings,
        load_runtime_cache, AcceptanceKind, AppliedAffixKind, ByteEncoding, ByteImportEncodings,
        CasingPath, ImportMode, LookupExplanation, RejectionReason, Severity, SourceDigests,
        MAX_AFF_BYTES, MAX_COMPOUND_SCALARS, MAX_DERIVED_CANDIDATES_PER_LOOKUP, MAX_DIC_BYTES,
        MAX_FLAGS_PER_ENTRY,
    };

    const AFFIXES: &str =
        "SET UTF-8\nFLAG UTF-8\nPFX A Y 1\nPFX A 0 un .\nSFX B Y 1\nSFX B y ies [^aeiou]y\n";

    #[test]
    fn runtime_flags_use_one_compact_machine_word() {
        assert_eq!(std::mem::size_of::<super::Flag>(), 8);
    }

    #[test]
    fn allocation_free_case_classification_matches_string_mappings() {
        for language in [super::CaseLanguage::Default, super::CaseLanguage::Turkic] {
            for character in ['A', 'a', 'İ', 'ı', 'ß', 'Σ', 'ς', 'ǅ', '1', '中'] {
                let lowercase = match (language, character) {
                    (super::CaseLanguage::Turkic, 'I') => "ı".to_owned(),
                    (super::CaseLanguage::Turkic, 'İ') => "i".to_owned(),
                    _ => character.to_lowercase().collect(),
                };
                let uppercase = match (language, character) {
                    (super::CaseLanguage::Turkic, 'i') => "İ".to_owned(),
                    (super::CaseLanguage::Turkic, 'ı') => "I".to_owned(),
                    _ => character.to_uppercase().collect(),
                };
                let original = character.to_string();

                assert_eq!(super::is_cased(character, language), lowercase != uppercase);
                assert_eq!(
                    super::is_uppercase(character, language),
                    original != lowercase
                );
                assert_eq!(
                    super::is_lowercase(character, language),
                    original != uppercase
                );
            }
        }
    }

    #[test]
    fn dictionary_stem_unescaping_borrows_no_op_inputs() {
        assert!(matches!(
            super::unescape_dictionary_stem("plain"),
            Cow::Borrowed("plain")
        ));
        assert_eq!(
            super::unescape_dictionary_stem(r"path\/name"),
            Cow::<str>::Owned("path/name".to_owned())
        );
    }

    #[test]
    fn compact_text_flag_order_matches_serialized_text_order() {
        let mut flags = ["B", "A\u{FE0F}", "Aa", "A", "Ab", "é", "😀"]
            .map(|flag| super::encode_text_flag(flag).expect("test flag is bounded"));
        flags.sort_unstable();
        let decoded =
            flags.map(|flag| super::decode_text_flag(flag).expect("encoded flag decodes"));

        assert_eq!(decoded, ["A", "Aa", "Ab", "A\u{FE0F}", "B", "é", "😀"]);
    }

    #[test]
    fn affix_edge_index_keeps_empty_and_matching_adds() {
        let result = import(
            "edges.aff",
            "PFX A Y 3\nPFX A 0 re .\nPFX A 0 un .\nPFX A 0 0 .\nSFX B Y 2\nSFX B 0 ing .\nSFX B 0 ed .\n",
            "edges.dic",
            "1\ndo/AB\n",
            ImportMode::Strict,
        )
        .expect("the edge-index fixture imports");
        let adds = result
            .dictionary()
            .candidate_affix_rules("redoing")
            .map(|rule| rule.add.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(adds, ["", "re", "ing"]);
    }

    #[test]
    fn imports_utf8_stems_and_evaluates_affixes_lazily() {
        let result = import(
            "test.aff",
            AFFIXES,
            "test.dic",
            "2\nkind/A\nparty/B\n",
            ImportMode::Strict,
        )
        .expect("the supported subset imports cleanly");
        let dictionary = result.dictionary();

        assert!(dictionary.contains("kind"));
        assert!(dictionary.contains("unkind"));
        assert!(dictionary.contains("parties"));
        assert!(!dictionary.contains("unkinds"));
        assert!(!dictionary.contains("partys"));
        assert!(!dictionary.contains("Strasse"));
    }

    #[test]
    fn explains_plain_affixed_compound_and_rejected_words() {
        let result = import(
            "explain.aff",
            "FORBIDDENWORD F\nNEEDAFFIX N\nONLYINCOMPOUND O\nSFX A Y 1\nSFX A 0 s/B .\nSFX B Y 1\nSFX B 0 ed .\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n",
            "explain.dic",
            "8\nplain\nroot/A\nhaus/C\ntür/C\nschlüssel/C\nbad/F\nneeds/N\ncomponent/O\n",
            ImportMode::Strict,
        )
        .expect("explanation fixture imports");
        let dictionary = result.dictionary();

        let plain = dictionary.explain("plain");
        let plain = plain.accepted().expect("plain word is accepted");
        assert!(matches!(plain.kind(), AcceptanceKind::Stem { stem } if stem == "plain"));

        let affixed = dictionary.explain("rootsed");
        let affixed = affixed.accepted().expect("affixed word is accepted");
        let AcceptanceKind::Affixed { stem, rules } = affixed.kind() else {
            panic!("diagnostic path must retain affix rules");
        };
        assert_eq!(stem, "root");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].kind(), AppliedAffixKind::Suffix);
        assert_eq!(rules[0].add(), "s");
        assert_eq!(rules[1].add(), "ed");

        let compound = dictionary.explain("haustürschlüssel");
        let compound = compound.accepted().expect("compound is accepted");
        let AcceptanceKind::Compound { components } = compound.kind() else {
            panic!("diagnostic path must retain compound components");
        };
        assert_eq!(
            components
                .iter()
                .map(super::CompoundComponent::stem)
                .collect::<Vec<_>>(),
            ["haus", "tür", "schlüssel"]
        );

        for (word, expected) in [
            (
                "bad",
                RejectionReason::ForbiddenStem {
                    stem: "bad".to_owned(),
                },
            ),
            (
                "needs",
                RejectionReason::NeedsAffix {
                    stem: "needs".to_owned(),
                },
            ),
            (
                "component",
                RejectionReason::OnlyInCompound {
                    stem: "component".to_owned(),
                },
            ),
            ("unknown", RejectionReason::NoDerivation),
        ] {
            let rejected = dictionary.explain(word);
            assert_eq!(
                rejected.rejected().expect("word is rejected").reason(),
                &expected
            );
        }
    }

    #[test]
    fn explanation_bounds_adversarial_compound_backtracking() {
        let result = import(
            "compound-trace.aff",
            "SET UTF-8\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n",
            "compound-trace.dic",
            "3\na/C\naa/C\naaa/C\n",
            ImportMode::Strict,
        )
        .expect("overlapping compound fixture imports");
        let dictionary = result.dictionary();
        let adversarial = format!("{}b", "a".repeat(40));

        assert!(!dictionary.contains(&adversarial));
        let explanation = dictionary.explain(&adversarial);

        assert_eq!(
            explanation
                .rejected()
                .expect("the unmatched suffix rejects the compound")
                .reason(),
            &RejectionReason::NoDerivation
        );
    }

    #[test]
    fn explanation_reports_case_fallback_without_changing_lookup() {
        let dictionary = import("case.aff", "", "case.dic", "1\nhouse\n", ImportMode::Strict)
            .expect("case fixture imports")
            .dictionary()
            .clone();

        for word in ["house", "HOUSE", "missing"] {
            assert_eq!(
                dictionary.contains(word),
                dictionary.explain(word).accepted().is_some(),
                "diagnostic and hot lookup outcomes agree for {word}"
            );
        }
        let accepted = dictionary
            .explain("HOUSE")
            .accepted()
            .cloned()
            .expect("fallback hit");
        assert_eq!(
            accepted.casing(),
            &CasingPath::CaseFallback {
                candidate: "house".to_owned()
            }
        );
        assert!(matches!(
            accepted.kind(),
            AcceptanceKind::Stem { stem } if stem == "house"
        ));
        assert!(matches!(
            dictionary.explain("missing"),
            LookupExplanation::Rejected(_)
        ));
    }

    #[test]
    fn retains_af_and_am_alias_metadata_through_the_runtime_cache() {
        let result = import(
            "aliases.aff",
            "AF 2\nAF AB\nAF C\nAM 2\nAM st:root\nAM st:other\nSFX B Y 1\nSFX B 0 s .\nSFX C Y 1\nSFX C 0 ed .\n",
            "aliases.dic",
            "2\nroot/1 1 po:noun\nother/2 2\n",
            ImportMode::Strict,
        )
        .expect("valid aliases import cleanly");

        assert!(result.dictionary().contains("roots"));
        assert!(result.dictionary().contains("othered"));
        assert!(result.diagnostics().is_empty());
        assert_eq!(
            result.dictionary().morphology.values_by_id(),
            vec!["st:root", "st:other", "po:noun"]
        );
        let ir = result.ir();
        assert_eq!(ir.morphology, ["st:root", "st:other", "po:noun"]);
        assert_eq!(ir.lexemes.len(), 2);
        assert_eq!(ir.suffixes.len(), 2);
        assert_eq!(ir.suffixes[0].add, "s");
        assert_eq!(ir.lexemes[1].morphology, [0, 2]);

        let cache = compile_runtime_cache(
            result.dictionary(),
            SourceDigests::from_source_bytes(b"aliases.aff", b"aliases.dic"),
        )
        .expect("metadata-bearing dictionary serializes");
        let loaded = load_runtime_cache(
            &cache,
            SourceDigests::from_source_bytes(b"aliases.aff", b"aliases.dic"),
        )
        .expect("metadata-bearing cache deserializes");
        assert_eq!(
            loaded.morphology.values_by_id(),
            result.dictionary().morphology.values_by_id()
        );
        assert_eq!(
            loaded.lexemes[0].morphology,
            result.dictionary().lexemes[0].morphology
        );
    }

    #[test]
    fn imports_key_and_map_for_suggestion_ranking_and_runtime_cache() {
        let result = import(
            "ranking.aff",
            "KEY qw|er\nMAP 1\nMAP áz\n",
            "ranking.dic",
            "4\ne\nw\na\nz\n",
            ImportMode::Strict,
        )
        .expect("ranking signals import cleanly");
        let dictionary = result.dictionary();

        assert_eq!(
            Suggester::new(dictionary, SuggestConfig::default())
                .with_ranking_signals(dictionary.ranking_signals())
                .suggest("q")
                .suggestions()[0]
                .word(),
            "w"
        );
        assert_eq!(
            Suggester::new(dictionary, SuggestConfig::default())
                .with_ranking_signals(dictionary.ranking_signals())
                .suggest("á")
                .suggestions()[0]
                .word(),
            "z"
        );
        assert_eq!(result.ir().keyboard.as_deref(), Some("qw|er"));
        assert_eq!(result.ir().character_maps, ["áz"]);

        let sources = SourceDigests::from_source_bytes(b"ranking.aff", b"ranking.dic");
        let cache = compile_runtime_cache(dictionary, sources).expect("ranking cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("ranking cache loads");
        assert_eq!(loaded.to_ir().keyboard.as_deref(), Some("qw|er"));
        assert_eq!(loaded.to_ir().character_maps, ["áz"]);
    }

    #[test]
    fn imports_long_flags_in_aliases_affixes_and_dictionary_entries() {
        let result = import(
            "long.aff",
            "FLAG long\nAF 2\nAF AaBb # root and suffix\nAF Cc\nNEEDAFFIX Aa\nSFX Bb N 1\nSFX Bb 0 s .\n",
            "long.dic",
            "2\nroot/1\nplain/2\n",
            ImportMode::Strict,
        )
        .expect("long flags import cleanly");

        assert!(!result.dictionary().contains("root"));
        assert!(result.dictionary().contains("roots"));
        assert!(result.dictionary().contains("plain"));
        assert!(result.diagnostics().is_empty());
    }

    #[test]
    fn imports_variation_selector_utf8_flags() {
        let result = import(
            "variation-selector-flags.aff",
            "FLAG UTF-8\nAF 1\nAF ☎️A\nPFX ☎️ N 1\nPFX ☎️ 0 tele .\nSFX A N 1\nSFX A 0 s .\n",
            "variation-selector-flags.dic",
            "1\nphone/1\n",
            ImportMode::Strict,
        )
        .expect("variation-selector UTF-8 flags import cleanly");

        assert!(result.dictionary().contains("telephone"));
        assert!(result.dictionary().contains("phones"));
    }

    #[test]
    fn rejects_a_standalone_utf8_variation_selector_flag() {
        let error = import(
            "invalid-variation-selector-flags.aff",
            "FLAG UTF-8\nPFX \u{fe0f} N 1\nPFX \u{fe0f} 0 tele .\n",
            "invalid-variation-selector-flags.dic",
            "1\nphone/\u{fe0f}\n",
            ImportMode::Strict,
        )
        .expect_err("a variation selector must modify a base flag scalar");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.directive() == "PFX" && diagnostic.severity() == Severity::Error
        }));
    }

    #[test]
    fn imports_numeric_flags_in_aliases_and_affix_continuations() {
        let result = import(
            "numeric.aff",
            "FLAG num\nAF 2\nAF 1,2\nAF 3\nNEEDAFFIX 1\nSFX 2 N 1\nSFX 2 0 s/2 .\nSFX 3 N 1\nSFX 3 0 ed .\n",
            "numeric.dic",
            "2\nroot/1\nplain/2\n",
            ImportMode::Strict,
        )
        .expect("numeric flags import cleanly");

        assert!(!result.dictionary().contains("root"));
        assert!(result.dictionary().contains("roots"));
        assert!(result.dictionary().contains("plain"));
        assert!(result.dictionary().contains("plained"));
        assert!(result.diagnostics().is_empty());
    }

    #[test]
    fn numeric_zero_flags_are_valid_affix_identifiers() {
        let affixes = "FLAG num\nSFX 0 N 1\nSFX 0 0 s .\n";
        let entries = "1\nword/0\n";
        let imported = import(
            "numeric-zero.aff",
            affixes,
            "numeric-zero.dic",
            entries,
            ImportMode::Strict,
        )
        .expect("zero is a valid numeric Hunspell flag");

        assert!(imported.dictionary().contains("words"));
        let sources = SourceDigests::from_source_bytes(affixes.as_bytes(), entries.as_bytes());
        let cache = compile_runtime_cache(imported.dictionary(), sources)
            .expect("numeric zero flags compile into the runtime cache");
        let loaded = load_runtime_cache(&cache, sources)
            .expect("numeric zero flags load from the runtime cache");
        assert!(loaded.contains("words"));
    }

    #[test]
    fn applies_bounded_negative_lookbehind_affix_conditions() {
        let result = import(
            "conditions.aff",
            "SFX A N 1\nSFX A 0 x (^|[^o])stem\nSFX B N 1\nSFX B 0 x (?<!i)[z]word\nSFX C N 1\nSFX C 0 x (^whole)\n",
            "conditions.dic",
            "6\nstem/A\nastem/A\nostem/A\nzword/B\nizword/B\nwhole/C\n",
            ImportMode::Strict,
        )
        .expect("bounded negative lookbehinds import cleanly");

        assert!(result.dictionary().contains("stemx"));
        assert!(result.dictionary().contains("astemx"));
        assert!(!result.dictionary().contains("ostemx"));
        assert!(result.dictionary().contains("zwordx"));
        assert!(!result.dictionary().contains("izwordx"));
        assert!(result.dictionary().contains("wholex"));
    }

    #[test]
    fn normalizes_iconv_and_ignore_before_every_lookup_strategy() {
        let result = import(
            "normalization.aff",
            "IGNORE \u{301}\nICONV 3\nICONV æ ae\nICONV -_ x\nICONV q 0\nSFX A Y 1\nSFX A 0 s .\n",
            "normalization.dic",
            "3\naer\nfinx\nword/A\n",
            ImportMode::Strict,
        )
        .expect("normalization directives import cleanly");
        let dictionary = result.dictionary();

        assert!(dictionary.contains("ær"));
        assert!(dictionary.contains("fin-"));
        assert!(dictionary.contains("wo\u{301}rds"));
        assert!(dictionary.contains("worqds"));
        assert!(!dictionary.contains("fins"));
    }

    #[test]
    fn normalizes_oconv_only_for_suggestion_output() {
        let result = import(
            "output-normalization.aff",
            "OCONV 3\nOCONV ae æ\nOCONV r_ 0\nOCONV x_ y\n",
            "output-normalization.dic",
            "1\naerx\n",
            ImportMode::Strict,
        )
        .expect("output conversion directives import cleanly");
        let dictionary = result.dictionary();

        assert!(dictionary.contains("aerx"));
        assert!(!dictionary.contains("æy"));
        assert_eq!(dictionary.normalize_output("aer"), "æ");
        assert_eq!(dictionary.normalize_output("aerx"), "æry");
    }

    #[test]
    fn malformed_oconv_is_a_strict_error() {
        let error = import(
            "malformed-output-normalization.aff",
            "OCONV 1\nOCONV source\n",
            "malformed-output-normalization.dic",
            "1\nword\n",
            ImportMode::Strict,
        )
        .expect_err("malformed OCONV must not be silently ignored");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.directive() == "OCONV" && diagnostic.severity() == Severity::Error
        }));
    }

    #[test]
    fn fullstrip_allows_an_affix_to_strip_the_entire_stem() {
        let result = import(
            "fullstrip.aff",
            "FULLSTRIP\nSFX A N 1\nSFX A word s .\n",
            "fullstrip.dic",
            "1\nword/A\n",
            ImportMode::Strict,
        )
        .expect("FULLSTRIP imports cleanly");

        assert!(result.dictionary().contains("s"));
    }

    #[test]
    fn full_stem_strips_require_fullstrip() {
        let result = import(
            "without-fullstrip.aff",
            "SFX A N 1\nSFX A word s .\n",
            "without-fullstrip.dic",
            "1\nword/A\n",
            ImportMode::Strict,
        )
        .expect("affix imports cleanly without FULLSTRIP");

        assert!(!result.dictionary().contains("s"));
    }

    #[test]
    fn malformed_iconv_or_ignore_are_strict_errors() {
        let error = import(
            "normalization.aff",
            "IGNORE\nICONV 1\nICONV only-source\n",
            "normalization.dic",
            "1\nword\n",
            ImportMode::Strict,
        )
        .expect_err("recognition-affecting directives must be complete");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            matches!(diagnostic.directive(), "ICONV" | "IGNORE")
                && diagnostic.severity() == Severity::Error
        }));
    }

    #[test]
    fn malformed_af_aliases_never_shift_dictionary_references() {
        let result = import(
            "aliases.aff",
            "AF 2\nAF A\nAF malformed extra\nSFX A Y 1\nSFX A 0 s .\n",
            "aliases.dic",
            "1\nroot/2\n",
            ImportMode::Lenient,
        )
        .expect("lenient imports retain only well-formed data");

        assert!(!result.dictionary().contains("roots"));
        assert!(result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "AF"
                && diagnostic.severity() == Severity::Error));
    }

    #[test]
    fn malformed_am_aliases_are_warning_diagnostics() {
        let result = import(
            "aliases.aff",
            "AM 1\nAM\n",
            "aliases.dic",
            "1\nword 1\n",
            ImportMode::Lenient,
        )
        .expect("lenient imports preserve the safe subset");

        assert!(result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "AM"
                && diagnostic.severity() == Severity::Warning));
    }

    #[test]
    fn exposes_only_stored_stems_as_suggestion_candidates() {
        let result = import(
            "test.aff",
            AFFIXES,
            "test.dic",
            "2\nkind/A\nparty/B\n",
            ImportMode::Strict,
        )
        .expect("the supported subset imports cleanly");
        let mut candidates = Vec::new();

        result.dictionary().visit_candidates(&mut |candidate| {
            candidates.push(candidate.to_owned());
            true
        });

        assert_eq!(candidates, ["kind", "party"]);
        assert!(!candidates.contains(&"parties".to_owned()));
    }

    #[test]
    fn suggestions_exclude_rejected_and_no_suggest_stems_after_cache_round_trip() {
        let aff = "FORBIDDENWORD F\nNEEDAFFIX N\nONLYINCOMPOUND O\nNOSUGGEST S\n";
        let dic = "5\nforbidden/F\nneeds/N\ncompound/O\nprivate/S\npublic\n";
        let imported = import(
            "suggestions.aff",
            aff,
            "suggestions.dic",
            dic,
            ImportMode::Strict,
        )
        .expect("the fixture imports");
        let source_digests = SourceDigests::from_source_bytes(aff.as_bytes(), dic.as_bytes());
        let cache = compile_runtime_cache(imported.dictionary(), source_digests)
            .expect("the cache compiles");
        let dictionary = load_runtime_cache(&cache, source_digests).expect("the cache loads");

        assert!(dictionary.contains("private"));
        for word in ["forbidden", "needs", "compound", "private"] {
            let result = Suggester::new(&dictionary, SuggestConfig::default()).suggest(word);
            assert!(
                !result
                    .suggestions()
                    .iter()
                    .any(|suggestion| suggestion.word() == word),
                "{word} must never be suggested"
            );
        }
        assert!(Suggester::new(&dictionary, SuggestConfig::default())
            .suggest("publi")
            .suggestions()
            .iter()
            .any(|suggestion| suggestion.word() == "public"));
        assert!(imported.ir().special_flags.no_suggest.is_some());
    }

    #[test]
    fn suggestions_expand_affixes_and_query_aligned_compounds_within_budgets() {
        let result = import(
            "german-class.aff",
            "SFX N Y 1\nSFX N 0 n .\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n",
            "german-class.dic",
            "3\nHäuser/N\nBahn/C\nHof/C\n",
            ImportMode::Strict,
        )
        .expect("the fixture imports");
        let dictionary = result.dictionary();
        let config = SuggestConfig {
            max_edit_distance: 2,
            max_candidates: 32,
            max_edit_cells: 2_000,
            ..SuggestConfig::default()
        };

        let affixed = Suggester::new(dictionary, config).suggest("Häusernn");
        let compound = Suggester::new(dictionary, config).suggest("BahnHoff");

        assert!(affixed
            .suggestions()
            .iter()
            .any(|suggestion| suggestion.word() == "Häusern"));
        assert!(compound
            .suggestions()
            .iter()
            .any(|suggestion| suggestion.word() == "BahnHof"));
        assert_eq!(affixed.completeness(), Completeness::Complete);
        assert_eq!(compound.completeness(), Completeness::Complete);
    }

    #[test]
    fn imports_replacement_rules_for_suggestion_ranking() {
        let result = import(
            "test.aff",
            "REP 1\nREP ^teh$ the\n",
            "test.dic",
            "2\ntea\nthe\n",
            ImportMode::Strict,
        )
        .expect("REP is a supported suggestion directive");
        let dictionary = result.dictionary();

        assert_eq!(dictionary.replacement_rules().len(), 1);
        assert_eq!(dictionary.replacement_rules()[0].from(), "teh");
        assert_eq!(dictionary.replacement_rules()[0].to(), "the");
        assert!(dictionary.replacement_rules()[0].at_word_start());
        assert!(dictionary.replacement_rules()[0].at_word_end());
        assert_eq!(
            Suggester::new(dictionary, SuggestConfig::default())
                .with_replacement_rules(dictionary.replacement_rules())
                .suggest("teh")
                .suggestions()[0]
                .word(),
            "the"
        );
    }

    #[test]
    fn counted_affix_sections_skip_blank_and_comment_lines_consistently() {
        let result = import(
            "counted-sections.aff",
            "COMPOUNDMIN 1\nREP 1\n# replacement comment\n\nREP teh the\nMAP 1\n  # map comment\n\nMAP aá\nCHECKCOMPOUNDPATTERN 1\n# compound pattern comment\n\nCHECKCOMPOUNDPATTERN x y\nCOMPOUNDRULE 1\n# compound rule comment\n\nCOMPOUNDRULE AB\nBREAK 1\n# break comment\n\nBREAK -\n",
            "counted-sections.dic",
            "4\nfoo/A\nbar/B\nthe\nword\n",
            ImportMode::Strict,
        )
        .expect("ignored lines do not consume declared section entries");

        assert!(result.diagnostics().is_empty());
        assert_eq!(result.ir().replacement_rules.len(), 1);
        assert_eq!(result.ir().character_maps, ["aá"]);
        assert_eq!(result.ir().compound.patterns.len(), 1);
        assert_eq!(result.ir().compound.rules.len(), 1);
        assert_eq!(result.ir().break_patterns.len(), 1);
        assert!(result.dictionary().contains("foobar"));
        assert!(result.dictionary().contains("foo-bar"));
    }

    #[test]
    fn malformed_replacement_rules_remain_warning_diagnostics() {
        let result = import(
            "test.aff",
            "REP 1\nREP missing-target\n",
            "test.dic",
            "1\nword\n",
            ImportMode::Strict,
        )
        .expect("suggestion-only malformed input does not change recognition");

        assert!(result.dictionary().replacement_rules().is_empty());
        assert!(result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "REP"
                && diagnostic.severity() == Severity::Warning));
    }

    #[test]
    fn informational_and_warning_directives_do_not_block_strict_import() {
        let result = import(
            "metadata.aff",
            "WARN W\nFORBIDWARN F\nONLYMAXDIFF\nHOME https://example.invalid\nNAME Test dictionary\nVERSION 1\n",
            "metadata.dic",
            "1\nword\n",
            ImportMode::Strict,
        )
        .expect("non-recognition directives are warnings");

        assert!(result.dictionary().contains("word"));
        assert!(result
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.severity() == Severity::Warning));
    }

    #[test]
    fn combines_one_cross_product_prefix_and_suffix_when_both_flags_apply() {
        let result = import(
            "test.aff",
            AFFIXES,
            "test.dic",
            "1\nparty/AB\n",
            ImportMode::Strict,
        )
        .expect("the supported subset imports cleanly");

        assert!(result.dictionary().contains("unparties"));
    }

    #[test]
    fn prevents_cross_product_when_a_header_does_not_opt_in() {
        let affixes = AFFIXES.replacen("SFX B Y", "SFX B N", 1);
        let result = import(
            "test.aff",
            &affixes,
            "test.dic",
            "1\nparty/AB\n",
            ImportMode::Strict,
        )
        .expect("the supported subset imports cleanly");

        assert!(!result.dictionary().contains("unparties"));
    }

    #[test]
    fn lenient_mode_retains_the_safe_subset_while_strict_mode_rejects_errors() {
        let affixes = "SET KOI8-R\nCOMPOUNDMIN 3\n";
        let lenient = import(
            "test.aff",
            affixes,
            "test.dic",
            "1\n東京\n",
            ImportMode::Lenient,
        )
        .expect("lenient import returns a safe subset");

        assert!(lenient.dictionary().contains("東京"));
        assert_eq!(lenient.diagnostics()[0].severity(), Severity::Error);
        assert!(import(
            "test.aff",
            affixes,
            "test.dic",
            "1\n東京\n",
            ImportMode::Strict
        )
        .is_err());
    }

    #[test]
    fn byte_import_decodes_iso_8859_1_from_the_affix_declaration() {
        let result = import_bytes(
            "latin1.aff",
            b"SET ISO8859-1\n",
            "latin1.dic",
            b"1\ncaf\xe9\n",
            ImportMode::Strict,
        )
        .expect("ISO-8859-1 bytes decode without replacement");

        assert!(result.dictionary().contains("café"));
    }

    #[test]
    fn byte_import_decodes_iso_8859_2_from_the_affix_declaration() {
        let result = import_bytes(
            "latin2.aff",
            b"SET ISO-8859-2\n",
            "latin2.dic",
            b"1\nza\xbf\xf3\xb3\xe6\n",
            ImportMode::Strict,
        )
        .expect("ISO-8859-2 bytes decode without replacement");

        assert!(result.dictionary().contains("zażółć"));
    }

    #[test]
    fn byte_import_uses_the_existing_utf8_default_without_set() {
        let result = import_bytes(
            "default.aff",
            b"SFX S N 1\nSFX S 0 s .\n",
            "default.dic",
            "1\nstraße/S\n".as_bytes(),
            ImportMode::Strict,
        )
        .expect("missing SET defaults to UTF-8");

        assert!(result.dictionary().contains("straßes"));
    }

    #[test]
    fn byte_import_accepts_a_utf8_bom_before_set() {
        let result = import_bytes(
            "bom.aff",
            b"\xef\xbb\xbfSET UTF-8\n",
            "bom.dic",
            "1\nMünchen\n".as_bytes(),
            ImportMode::Strict,
        )
        .expect("a UTF-8 BOM is normalized before parsing the affix file");

        assert!(result.dictionary().contains("München"));
    }

    #[test]
    fn string_import_accepts_a_utf8_bom_in_both_sources() {
        let result = import(
            "bom.aff",
            "\u{feff}SET UTF-8\n",
            "bom.dic",
            "\u{feff}2\nhello\nworld\n",
            ImportMode::Strict,
        )
        .expect("leading Unicode BOMs are normalized before parsing either source");

        assert!(result.diagnostics().is_empty());
        assert!(result.dictionary().contains("hello"));
        assert!(result.dictionary().contains("world"));
        assert!(!result.dictionary().contains("\u{feff}2"));
    }

    #[test]
    fn byte_import_strips_a_dictionary_bom_without_disabling_count_validation() {
        let result = import_bytes(
            "bom.aff",
            b"SET UTF-8\n",
            "bom.dic",
            b"\xef\xbb\xbf1\nhello\nworld\n",
            ImportMode::Strict,
        )
        .expect("a dictionary BOM does not hide a count mismatch");

        assert!(result.diagnostics().iter().any(|diagnostic| {
            diagnostic.directive() == "count"
                && diagnostic.message() == "declared 1 entries but parsed 2"
        }));
        assert!(result.dictionary().contains("hello"));
        assert!(result.dictionary().contains("world"));
        assert!(!result.dictionary().contains("\u{feff}1"));
    }

    #[test]
    fn byte_import_allows_a_reviewed_mixed_encoding_pair() {
        let result = import_bytes_with_encodings(
            "mixed.aff",
            b"SET ISO-8859-1\n",
            "mixed.dic",
            "1\ncafé\n".as_bytes(),
            ByteImportEncodings::new(ByteEncoding::Iso8859_1, ByteEncoding::Utf8),
            ImportMode::Strict,
        )
        .expect("the per-file override decodes the reviewed mixed pair");

        assert!(result.dictionary().contains("café"));
    }

    #[test]
    fn byte_import_allows_a_reviewed_utf8_affix_with_iso_8859_2_fallback() {
        let result = import_bytes_with_encodings(
            "mixed-utf8.aff",
            b"SET UTF-8\n# legacy byte: \xe1\nSFX S N 1\x85SFX S 0 s .\n",
            "mixed-utf8.dic",
            b"1\nword/S\n",
            ByteImportEncodings::new(ByteEncoding::Utf8WithIso8859_2Fallback, ByteEncoding::Utf8),
            ImportMode::Strict,
        )
        .expect("the reviewed fallback retains UTF-8 and legacy affix bytes");

        assert!(result.dictionary().contains("words"));
    }

    #[test]
    fn byte_import_rejects_unsupported_set_without_parsing_a_subset() {
        let error = import_bytes(
            "unsupported.aff",
            b"SET KOI8-R\n",
            "unsupported.dic",
            b"1\nword\n",
            ImportMode::Strict,
        )
        .expect_err("unsupported byte encodings are strict import failures");

        assert_eq!(error.diagnostics()[0].source(), "unsupported.aff");
        assert_eq!(error.diagnostics()[0].line(), 1);
        assert_eq!(error.diagnostics()[0].directive(), "SET");
    }

    #[test]
    fn byte_import_rejects_malformed_utf8_with_a_source_diagnostic() {
        let error = import_bytes(
            "utf8.aff",
            b"SET UTF-8\n",
            "utf8.dic",
            b"1\nword\n\xff",
            ImportMode::Strict,
        )
        .expect_err("malformed UTF-8 must not be replaced");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.source() == "utf8.dic"
                && diagnostic.line() == 3
                && diagnostic.directive() == "encoding"
        }));
    }

    #[test]
    fn byte_import_rejects_an_oversized_affix_before_scanning_or_decoding_it() {
        let oversized_affix = vec![0xff; MAX_AFF_BYTES + 1];
        let error = import_bytes(
            "too-large.aff",
            &oversized_affix,
            "small.dic",
            b"1\nword\n",
            ImportMode::Strict,
        )
        .expect_err("the raw affix limit is enforced before decoding");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.source() == "too-large.aff"
                && diagnostic.directive() == "input"
                && diagnostic.severity() == Severity::Error
        }));
        assert!(!error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "encoding"));
    }

    #[test]
    fn byte_import_rejects_an_oversized_dictionary_before_decoding_it() {
        let oversized_dictionary = vec![b'x'; MAX_DIC_BYTES + 1];
        let error = import_bytes(
            "small.aff",
            b"SET UTF-8\n",
            "too-large.dic",
            &oversized_dictionary,
            ImportMode::Strict,
        )
        .expect_err("the raw dictionary limit is enforced before decoding");

        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.source() == "too-large.dic"
                && diagnostic.directive() == "input"
                && diagnostic.severity() == Severity::Error
        }));
    }

    #[test]
    fn complex_prefixes_allow_two_prefixes_and_one_suffix_for_rtl_forms() {
        let affixes = "COMPLEXPREFIXES\nPFX A Y 1\nPFX A 0 م/B .\nPFX B Y 1\nPFX B 0 ال .\nSFX C Y 1\nSFX C 0 ات .\n";
        let entries = "1\nكتب/AC\n";
        let without_marker = import(
            "simple-prefixes.aff",
            affixes
                .strip_prefix("COMPLEXPREFIXES\n")
                .expect("known marker"),
            "rtl.dic",
            entries,
            ImportMode::Strict,
        )
        .expect("single-prefix compatibility fixture imports");
        assert!(
            !without_marker.dictionary().contains("المكتبات"),
            "a second prefix is never approximated without COMPLEXPREFIXES"
        );

        let imported = import(
            "complex-prefixes.aff",
            affixes,
            "rtl.dic",
            entries,
            ImportMode::Strict,
        )
        .expect("COMPLEXPREFIXES imports cleanly");
        assert!(imported.dictionary().contains("المكتبات"));

        let sources = SourceDigests::from_source_bytes(affixes.as_bytes(), entries.as_bytes());
        let cache = compile_runtime_cache(imported.dictionary(), sources)
            .expect("complex prefixes compile into the runtime cache");
        let loaded = load_runtime_cache(&cache, sources)
            .expect("complex prefixes load from the runtime cache");
        assert!(loaded.contains("المكتبات"));
    }

    #[test]
    fn resource_limits_produce_diagnostics_without_panicking() {
        let excessive_flags = "A".repeat(MAX_FLAGS_PER_ENTRY + 1);
        let dictionary = format!("1\nword/{excessive_flags}\n");
        let result = import("test.aff", "", "test.dic", &dictionary, ImportMode::Lenient)
            .expect("lenient import returns diagnostics");

        assert!(result
            .diagnostics()
            .iter()
            .any(|item| item.message().contains("4096-flag importer limit")));
        assert!(!result.dictionary().contains("word"));
    }

    #[test]
    fn reports_malformed_rules_and_count_mismatches_without_panicking() {
        let result = import(
            "test.aff",
            "PFX A Y 2\nPFX A 0 re [abc\n",
            "test.dic",
            "3\nword/A\n",
            ImportMode::Lenient,
        )
        .expect("lenient import returns diagnostics");

        assert!(result
            .diagnostics()
            .iter()
            .any(|item| item.directive() == "PFX" && item.line() == 2));
        assert!(result
            .diagnostics()
            .iter()
            .any(|item| item.directive() == "count"));
    }

    #[test]
    fn deterministic_adversarial_import_corpus_never_panics() {
        let excessive_flags = "A".repeat(MAX_FLAGS_PER_ENTRY + 1);
        let excessive_condition = ".".repeat(257);
        let affixes = [
            String::new(),
            "\0\n\u{feff}\n".to_owned(),
            "PFX A Y 18446744073709551616\n".to_owned(),
            "PFX A Y 2\nPFX A 0 re [\n".to_owned(),
            format!("SFX A N 1\nSFX A 0 s {excessive_condition}\n"),
            "SFX A Y 1\nPFX A 0 re .\n".to_owned(),
            "COMPOUNDMIN 0\nFORBIDDENWORD AB\n".to_owned(),
        ];
        let dictionaries = [
            String::new(),
            "\0\n".to_owned(),
            "18446744073709551616\nword\n".to_owned(),
            "2\nword/\n\n".to_owned(),
            format!("1\nword/{excessive_flags}\n"),
            "1\n/ABC\n".to_owned(),
            "1\n東京/A\n".to_owned(),
        ];

        for (aff_index, aff) in affixes.iter().enumerate() {
            for (dictionary_index, dictionary) in dictionaries.iter().enumerate() {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    let imported = import(
                        "adversarial.aff",
                        aff,
                        "adversarial.dic",
                        dictionary,
                        ImportMode::Lenient,
                    )
                    .expect("lenient mode always returns a safe subset");
                    for query in ["", "word", "東京", "wordword", "\0"] {
                        let _ = imported.dictionary().contains(query);
                    }
                }));
                assert!(
                    outcome.is_ok(),
                    "adversarial import case aff={aff_index}, dictionary={dictionary_index} panicked"
                );
            }
        }
    }

    #[test]
    fn compound_evaluation_rejects_overlong_queries_before_indexing_them() {
        let imported = import(
            "test.aff",
            "COMPOUNDFLAG M\nCOMPOUNDMIN 1\n",
            "test.dic",
            "2\na/M\nb/M\n",
            ImportMode::Strict,
        )
        .expect("compound dictionary imports");
        let query = "ab".repeat(MAX_COMPOUND_SCALARS);

        assert!(!imported.dictionary().contains(&query));
    }

    #[test]
    fn compound_minimum_zero_uses_hunspells_one_scalar_floor() {
        let imported = import(
            "minimum.aff",
            "COMPOUNDFLAG C\nCOMPOUNDMIN 0\n",
            "minimum.dic",
            "2\na/C\nb/C\n",
            ImportMode::Strict,
        )
        .expect("COMPOUNDMIN 0 is clamped to one");

        assert!(imported.dictionary().contains("ab"));
    }

    #[test]
    fn compound_safeguards_reject_forbidden_boundaries_and_allow_bounded_syllables() {
        let affixes = "COMPOUNDFLAG C\nCOMPOUNDMIN 1\nCOMPOUNDFORBIDFLAG F\nCOMPOUNDWORDMAX 2 C\nCOMPOUNDSYLLABLE 1 a\nCHECKCOMPOUNDDUP\nCHECKCOMPOUNDCASE\nCHECKCOMPOUNDTRIPLE\nCHECKCOMPOUNDREP\nFORCEUCASE U\nCHECKCOMPOUNDPATTERN 1\nCHECKCOMPOUNDPATTERN foo/A bar/B\nREP 1\nREP quxbar known\n";
        let entries = "13\nfoo/CA\nbar/CB\nBar/CB\nox/C\nbad/CF\nmain/C\nMain/C\nstreet/CU\na/C\nb/C\nc/C\nknown\nqux/C\n";
        let imported = import(
            "safeguards.aff",
            affixes,
            "safeguards.dic",
            entries,
            ImportMode::Strict,
        )
        .expect("compound safeguards import in strict mode");
        let dictionary = imported.dictionary();

        assert!(
            !dictionary.contains("foobar"),
            "the flagged pattern blocks foo|bar"
        );
        assert!(
            !dictionary.contains("badfoo"),
            "forbid flag removes bad from compounds"
        );
        assert!(
            dictionary.contains("foobad"),
            "the Hunspell forbid flag permits a direct final component"
        );
        assert!(
            !dictionary.contains("foofoo"),
            "adjacent duplicate components are rejected"
        );
        assert!(
            !dictionary.contains("fooox"),
            "a boundary triple is rejected"
        );
        assert!(
            !dictionary.contains("fooBar"),
            "uppercase at a boundary is rejected"
        );
        assert!(
            !dictionary.contains("mainstreet"),
            "FORCEUCASE requires capitalization"
        );
        assert!(dictionary.contains("Mainstreet"));
        assert!(
            dictionary.contains("abc"),
            "one-syllable compounds may exceed word max"
        );
        assert!(
            !dictionary.contains("quxbar"),
            "REP correction to a plain word blocks compounding"
        );

        let sources = SourceDigests::from_source_bytes(affixes.as_bytes(), entries.as_bytes());
        let cache = compile_runtime_cache(dictionary, sources).expect("safeguards cache compiles");
        let loaded = load_runtime_cache(&cache, sources).expect("safeguards cache loads");
        assert!(!loaded.contains("foofoo"));
        assert!(loaded.contains("Mainstreet"));
    }

    #[test]
    fn compound_boundary_reductions_are_bounded_and_explicit() {
        let simplified = import(
            "simplified-triple.aff",
            "COMPOUNDFLAG C\nCOMPOUNDMIN 1\nCHECKCOMPOUNDTRIPLE\nSIMPLIFIEDTRIPLE\n",
            "simplified-triple.dic",
            "2\nSchiff/C\nfahrt/C\n",
            ImportMode::Strict,
        )
        .expect("simplified triple directives import");
        assert!(!simplified.dictionary().contains("Schifffahrt"));
        assert!(simplified.dictionary().contains("Schiffahrt"));

        let pattern = import(
            "compound-pattern.aff",
            "COMPOUNDFLAG C\nCOMPOUNDMIN 1\nCHECKCOMPOUNDPATTERN 1\nCHECKCOMPOUNDPATTERN ff f ff\n",
            "compound-pattern.dic",
            "2\nSchiff/C\nfahrt/C\n",
            ImportMode::Strict,
        )
        .expect("compound replacement pattern imports");
        assert!(pattern.dictionary().contains("Schiffahrt"));
    }

    #[test]
    fn compound_patterns_allow_flag_only_boundaries_in_long_flag_dictionaries() {
        let imported = import(
            "long-pattern.aff",
            "FLAG long\nCOMPOUNDFLAG Cc\nCOMPOUNDMIN 1\nCHECKCOMPOUNDPATTERN 1\nCHECKCOMPOUNDPATTERN /Aa /Bb\n",
            "long-pattern.dic",
            "2\nleft/CcAa\nright/CcBb\n",
            ImportMode::Strict,
        )
        .expect("flag-only compound pattern imports");

        assert!(!imported.dictionary().contains("leftright"));
    }

    #[test]
    fn compound_rules_require_the_documented_component_flag_order() {
        let imported = import(
            "test.aff",
            "COMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE AB\n",
            "test.dic",
            "2\nHaus/A\nTür/B\n",
            ImportMode::Strict,
        )
        .expect("two-component compound rule imports");

        assert!(imported.dictionary().contains("HausTür"));
        assert!(!imported.dictionary().contains("TürHaus"));
    }

    #[test]
    fn compound_rules_support_bounded_three_component_patterns() {
        let imported = import(
            "test.aff",
            "COMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE ABC\n",
            "test.dic",
            "3\nBahn/A\nHof/B\nStraße/C\n",
            ImportMode::Strict,
        )
        .expect("three-component compound rule imports");

        assert!(imported.dictionary().contains("BahnHofStraße"));
        assert!(!imported.dictionary().contains("BahnStraßeHof"));
        assert!(!imported.dictionary().contains("BahnHof"));
    }

    #[test]
    fn compound_positions_and_compound_only_stems_are_enforced() {
        let imported = import(
            "test.aff",
            "COMPOUNDBEGIN B\nCOMPOUNDMIDDLE M\nCOMPOUNDEND E\nONLYINCOMPOUND O\nCOMPOUNDMIN 1\n",
            "test.dic",
            "4\nBahn/B\nHof/M\nStraße/E\nTeil/BO\n",
            ImportMode::Strict,
        )
        .expect("positioned compound directives import");

        let dictionary = imported.dictionary();
        assert!(!dictionary.contains("Teil"));
        assert!(dictionary.contains("BahnStraße"));
        assert!(dictionary.contains("BahnHofStraße"));
        assert!(dictionary.contains("TeilStraße"));
        assert!(!dictionary.contains("HofStraße"));
        assert!(!dictionary.contains("BahnHof"));
    }

    #[test]
    fn literal_break_characters_join_one_recognized_boundary() {
        let imported = import(
            "test.aff",
            "BREAK 2\nBREAK -\nBREAK .\n",
            "test.dic",
            "3\nE\nMail\nAdresse\n",
            ImportMode::Strict,
        )
        .expect("literal breaks import");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("E-Mail"));
        assert!(dictionary.contains("Mail.Adresse"));
        assert!(
            !dictionary.contains("E-Mail.Adresse"),
            "BREAK matching is non-recursive"
        );
        assert!(!dictionary.contains("E-Mail.unbekannt"));
        assert!(!dictionary.contains(".Adresse"));
    }

    #[test]
    fn compound_permit_affixes_are_limited_to_their_declared_positions() {
        let imported = import(
            "test.aff",
            "COMPOUNDBEGIN B\nCOMPOUNDEND E\nCOMPOUNDPERMITFLAG P\nCOMPOUNDMIN 1\nSFX A N 1\nSFX A 0 s/P .\nSFX C N 1\nSFX C 0 x .\n",
            "test.dic",
            "3\nroot/BA\nplain/BC\nend/E\n",
            ImportMode::Strict,
        )
        .expect("compound permit directives import");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("rootsend"));
        assert!(!dictionary.contains("plainxend"));
    }

    #[test]
    fn checksharps_accepts_only_the_keepcase_ss_uppercase_form() {
        let imported = import(
            "test.aff",
            "CHECKSHARPS\nKEEPCASE K\n",
            "test.dic",
            "2\nStraße/K\nMaße\n",
            ImportMode::Strict,
        )
        .expect("CHECKSHARPS imports");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("Straße"));
        assert!(dictionary.contains("STRASSE"));
        assert!(!dictionary.contains("STRAẞE"));
        assert!(!dictionary.contains("MASSE"));
    }

    #[test]
    fn lang_applies_hunspell_capitalization_fallbacks_and_turkic_i_casing() {
        let imported = import(
            "test.aff",
            "LANG tr_TR\nKEEPCASE K\n",
            "test.dic",
            "3\ni\nışık\nAnkara/K\n",
            ImportMode::Strict,
        )
        .expect("LANG imports");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("İ"));
        assert!(dictionary.contains("IŞIK"));
        assert!(dictionary.contains("Ankara"));
        assert!(!dictionary.contains("ANKARA"));
    }

    #[test]
    fn lang_uses_default_unicode_casing_outside_turkic_languages() {
        let imported = import(
            "test.aff",
            "LANG pt_PT\n",
            "test.dic",
            "2\nword\nışık\n",
            ImportMode::Strict,
        )
        .expect("LANG imports");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("WORD"));
        assert!(!dictionary.contains("IŞIK"));
    }

    #[test]
    fn capitalization_fallback_applies_without_lang() {
        let imported = import("test.aff", "", "test.dic", "1\nword\n", ImportMode::Strict)
            .expect("dictionary imports");

        assert!(imported.dictionary().contains("Word"));
        assert!(imported.dictionary().contains("WORD"));
    }

    #[test]
    fn wordchars_are_preserved_as_tokenization_metadata() {
        let imported = import(
            "test.aff",
            "WORDCHARS ß-.\n",
            "test.dic",
            "1\nWort\n",
            ImportMode::Strict,
        )
        .expect("WORDCHARS imports");

        assert_eq!(
            imported.dictionary().word_characters().collect::<Vec<_>>(),
            ['-', '.', 'ß']
        );
    }

    #[test]
    fn break_patterns_support_anchors_and_bounded_multiscalar_splits() {
        let imported = import(
            "test.aff",
            "BREAK 3\nBREAK --\nBREAK ^'\nBREAK '$\n",
            "test.dic",
            "3\nfoo\nbar\nword\n",
            ImportMode::Strict,
        )
        .expect("anchored and multi-scalar BREAK patterns import");
        let dictionary = imported.dictionary();

        assert!(dictionary.contains("foo--bar"));
        assert!(dictionary.contains("'word"));
        assert!(dictionary.contains("word'"));
        assert!(
            !dictionary.contains("foo--bar--foo"),
            "BREAK matching is non-recursive"
        );

        let disabled = import(
            "disabled-break.aff",
            "BREAK 0\n",
            "disabled-break.dic",
            "2\nfoo\nbar\n",
            ImportMode::Strict,
        )
        .expect("BREAK 0 disables the default patterns");

        assert!(!disabled.dictionary().contains("foo-bar"));
    }

    #[test]
    fn default_break_patterns_join_hyphenated_words() {
        let imported = import(
            "test.aff",
            "",
            "test.dic",
            "2\nE\nMail\n",
            ImportMode::Strict,
        )
        .expect("the default BREAK patterns are supported");

        assert!(imported.dictionary().contains("E-Mail"));
        assert!(imported.dictionary().contains("-Mail"));
    }

    #[test]
    fn iconv_uses_single_pass_longest_match_and_word_start_anchors() {
        let imported = import(
            "test.aff",
            "ICONV 3\nICONV ab x\nICONV x y\nICONV _pre 0\n",
            "test.dic",
            "2\nx\nword\n",
            ImportMode::Strict,
        )
        .expect("ICONV rules import");

        assert!(imported.dictionary().contains("ab"));
        assert!(!imported.dictionary().contains("y"));
        assert!(imported.dictionary().contains("preword"));
    }

    #[test]
    fn unicode_digit_flags_are_not_treated_as_af_aliases() {
        let imported = import(
            "test.aff",
            "AF 1\nAF A\n",
            "test.dic",
            "1\nword/٣\n",
            ImportMode::Strict,
        )
        .expect("Unicode flag remains a literal flag");

        assert!(imported.dictionary().contains("word"));
    }

    #[test]
    fn parenthesized_compound_rules_preserve_one_flag_per_group() {
        for (affixes, entries, compound) in [
            (
                "FLAG UTF-8\nCOMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE (A)(B)\n",
                "2\nleft/A\nright/B\n",
                "leftright",
            ),
            (
                "FLAG long\nCOMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE (aa)(bb)\n",
                "2\nleft/aa\nright/bb\n",
                "leftright",
            ),
            (
                "FLAG num\nCOMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE (1)(2)\n",
                "2\nleft/1\nright/2\n",
                "leftright",
            ),
        ] {
            let imported = import("test.aff", affixes, "test.dic", entries, ImportMode::Strict)
                .expect("one grouped flag per component imports");
            assert!(imported.dictionary().contains(compound));
        }

        let error = import(
            "test.aff",
            "COMPOUNDRULE 1\nCOMPOUNDRULE (A)(B\n",
            "test.dic",
            "1\nword\n",
            ImportMode::Strict,
        )
        .expect_err("unbalanced groups remain explicit errors");
        assert!(error.diagnostics().iter().any(|diagnostic| {
            diagnostic.directive() == "COMPOUNDRULE" && diagnostic.message().contains("balanced")
        }));
    }

    #[test]
    fn dictionary_comments_escaped_delimiters_and_empty_morphology_flags_import() {
        let imported = import(
            "test.aff",
            "FLAG long\n",
            "test.dic",
            "2\n/ provenance comment\nCO/ po:abbrev\ng\\/cm³\n",
            ImportMode::Strict,
        )
        .expect("reviewed dictionary conventions import without approximation");

        assert!(imported.dictionary().contains("CO"));
        assert!(imported.dictionary().contains("g/cm³"));
        assert!(imported.diagnostics().is_empty());
    }

    #[test]
    fn compound_rule_expansion_is_bounded_before_large_allocation() {
        let pattern = "A*".repeat(16);
        let affixes = format!("COMPOUNDRULE 1\nCOMPOUNDRULE {pattern}\n");
        let result = import(
            "test.aff",
            &affixes,
            "test.dic",
            "1\na/A\n",
            ImportMode::Lenient,
        )
        .expect("lenient import returns the safe subset");

        assert!(result.diagnostics().iter().any(|diagnostic| {
            diagnostic.directive() == "COMPOUNDRULE"
                && diagnostic.message().contains("per-rule limit")
        }));
    }

    #[test]
    fn homonym_flags_are_evaluated_independently() {
        let imported = import(
            "test.aff",
            "NEEDAFFIX N\n",
            "test.dic",
            "2\nfoo/N\nfoo/S\n",
            ImportMode::Strict,
        )
        .expect("homonym fixture imports");

        assert!(imported.dictionary().contains("foo"));
    }

    #[test]
    fn continuation_needaffix_and_onlyincompound_flags_are_enforced() {
        let imported = import(
            "test.aff",
            "NEEDAFFIX N\nONLYINCOMPOUND O\nCOMPOUNDBEGIN B\nCOMPOUNDEND E\nCOMPOUNDMIN 1\nSFX A N 1\nSFX A 0 x/N .\nSFX N N 1\nSFX N 0 y .\nSFX C N 1\nSFX C 0 z/O .\n",
            "test.dic",
            "2\nroot/AB\nend/EC\n",
            ImportMode::Strict,
        )
        .expect("continuation flags import");
        let dictionary = imported.dictionary();

        assert!(!dictionary.contains("rootx"));
        assert!(dictionary.contains("rootxy"));
        assert!(!dictionary.contains("endz"));
        assert!(dictionary.contains("rootendz"));
    }

    #[test]
    fn affix_composition_is_limited_to_one_prefix_and_two_suffixes() {
        let imported = import(
            "test.aff",
            "PFX A Y 1\nPFX A 0 un/D .\nPFX D Y 1\nPFX D 0 re .\nSFX B Y 1\nSFX B 0 s/C .\nSFX C Y 1\nSFX C 0 x/D .\nSFX D Y 1\nSFX D 0 y .\n",
            "test.dic",
            "1\nword/AB\n",
            ImportMode::Strict,
        )
        .expect("composition fixture imports");
        let dictionary = imported.dictionary();

        assert!(dictionary.contains("unword"));
        assert!(!dictionary.contains("reunword"));
        assert!(dictionary.contains("wordsx"));
        assert!(!dictionary.contains("wordsxy"));
    }

    #[test]
    fn reverse_affix_candidates_have_a_per_lookup_limit() {
        let mut dictionary = String::new();
        for index in 0..=MAX_DERIVED_CANDIDATES_PER_LOOKUP {
            writeln!(dictionary, "word{index}/A").expect("writing to String does not fail");
        }
        let dictionary = format!("{}\n{dictionary}", MAX_DERIVED_CANDIDATES_PER_LOOKUP + 1);
        let imported = import(
            "test.aff",
            "SFX A N 1\nSFX A 0 0 .\n",
            "test.dic",
            &dictionary,
            ImportMode::Strict,
        )
        .expect("large affix class imports");

        let dictionary = imported.dictionary();
        assert!(!dictionary.contains("not-a-generated-form"));
        assert!(
            dictionary
                .derived_candidate_indices("not-a-generated-form")
                .is_some_and(|candidates| candidates.is_empty()),
            "an empty-add rule should not pull its entire flag class into a miss lookup"
        );
    }

    #[test]
    fn reverse_candidates_preserve_chained_empty_add_rules() {
        let imported = import(
            "test.aff",
            "SFX A Y 1\nSFX A x 0/B .\nSFX B Y 1\nSFX B x 0 .\n",
            "test.dic",
            "1\nrootxx/A\n",
            ImportMode::Strict,
        )
        .expect("empty-add continuation fixture imports");

        assert!(imported.dictionary().contains("root"));
    }

    #[test]
    fn compound_rule_quantifiers_expand_with_a_bounded_component_limit() {
        let result = import(
            "test.aff",
            "COMPOUNDMIN 1\nCOMPOUNDRULE 1\nCOMPOUNDRULE A*B\n",
            "test.dic",
            "2\na/A\nb/B\n",
            ImportMode::Strict,
        )
        .expect("bounded quantifier syntax imports");

        assert!(result.dictionary().contains("aab"));
    }

    #[test]
    fn retains_affix_morphology_alongside_continuation_flags() {
        let result = import(
            "test.aff",
            "SFX A N 1\nSFX A 0 s/B . DS:plural\n",
            "test.dic",
            "1\nword/A\n",
            ImportMode::Strict,
        )
        .expect("affix metadata is retained");

        assert!(result.dictionary().contains("words"));
        assert!(result.diagnostics().is_empty());
        assert_eq!(
            result.dictionary().morphology.values_by_id(),
            vec!["DS:plural"]
        );
        assert_eq!(result.dictionary().suffixes[0].morphology.len(), 1);
    }

    #[test]
    fn continuation_classes_enable_an_additional_affix_transformation() {
        let result = import(
            "test.aff",
            "SFX A N 1\nSFX A 0 x/B .\nSFX B N 1\nSFX B 0 y .\n",
            "test.dic",
            "1\nroot/A\n",
            ImportMode::Strict,
        )
        .expect("continuation classes are supported");

        assert!(result.dictionary().contains("rootx"));
        assert!(result.dictionary().contains("rootxy"));
    }

    #[test]
    fn affix_rules_do_not_chain_without_a_continuation_or_cross_product() {
        let result = import(
            "test.aff",
            "SFX A N 1\nSFX A 0 x .\nSFX B N 1\nSFX B 0 y .\n",
            "test.dic",
            "1\nroot/AB\n",
            ImportMode::Strict,
        )
        .expect("basic affixes are supported");

        assert!(result.dictionary().contains("rootx"));
        assert!(result.dictionary().contains("rooty"));
        assert!(!result.dictionary().contains("rootxy"));
    }

    #[test]
    fn pathological_affix_branching_has_a_deterministic_lookup_budget() {
        let mut rules = String::new();
        for index in 0..100 {
            writeln!(rules, "SFX A 0 x{index} .").expect("writing to String does not fail");
        }
        let affixes = format!("SFX A N 100\n{rules}");
        let result = import(
            "test.aff",
            &affixes,
            "test.dic",
            "1\nroot/A\n",
            ImportMode::Strict,
        )
        .expect("bounded valid rules import");

        assert!(result.dictionary().contains("root"));
        assert!(!result.dictionary().contains("not-a-generated-form"));
    }

    #[test]
    fn advanced_flags_and_simple_compounds_follow_the_documented_contract() {
        let result = import(
            "test.aff",
            "CIRCUMFIX C\nFORBIDDENWORD F\nNEEDAFFIX N\nKEEPCASE K\nCOMPOUNDFLAG M\nCOMPOUNDMIN 3\nPFX A Y 1\nPFX A 0 un/C .\nSFX B Y 1\nSFX B 0 s/C .\nPFX D N 1\nPFX D 0 re .\n",
            "test.dic",
            "6\nword/AB\nfix/DN\nbad/AF\nHaus/M\ntür/M\nOAuth/K\n",
            ImportMode::Strict,
        )
        .expect("advanced flags are supported");
        let dictionary = result.dictionary();

        assert!(dictionary.contains("word"));
        assert!(!dictionary.contains("unword"));
        assert!(!dictionary.contains("words"));
        assert!(dictionary.contains("unwords"));
        assert!(!dictionary.contains("fix"));
        assert!(dictionary.contains("refix"));
        assert!(!dictionary.contains("bad"));
        assert!(!dictionary.contains("unbad"));
        assert!(dictionary.contains("Haustür"));
        assert!(!dictionary.contains("HausHa"));
        assert!(dictionary.contains("OAuth"));
        assert!(!dictionary.contains("oauth"));
    }

    #[test]
    fn imported_dictionaries_are_safe_to_share_across_threads() {
        let dictionary = Arc::new(
            import(
                "test.aff",
                AFFIXES,
                "test.dic",
                "1\nparty/AB\n",
                ImportMode::Strict,
            )
            .expect("the supported subset imports cleanly")
            .dictionary()
            .clone(),
        );
        let workers = (0..4)
            .map(|_| {
                let dictionary = Arc::clone(&dictionary);
                thread::spawn(move || dictionary.contains("unparties"))
            })
            .collect::<Vec<_>>();

        assert!(workers
            .into_iter()
            .all(|worker| worker.join().expect("worker does not panic")));
    }
}
