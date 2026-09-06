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
mod compound;
mod explanation;
mod ir;
mod model;
mod parse;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, OnceLock};

use encoding_rs::ISO_8859_2;
use ferrolex_compiler::{
    AffixKindIr, AffixRuleIr, BreakPatternIr, CaseLanguageIr, CompoundConfigIr, CompoundPatternIr,
    CompoundSyllableLimitIr, ConditionAtomIr, ConditionIr, FlagIr, LexemeIr, ReplacementRuleIr,
    SpecialFlagsIr,
};
use ferrolex_core::{CandidateIndex, Dictionary};

pub(crate) use ir::{
    affix_rule_to_ir, break_pattern_to_ir, case_language_to_ir, compound_to_ir, flag_mode_to_ir,
    input_conversion_to_ir, lexeme_to_ir, replacement_rule_to_ir, special_flags_to_ir,
};
pub(crate) use model::{
    case_pattern, decode_text_flag, encode_text_flag, has_flag, initial_case_for_language,
    lowercase_for_language, AffixKind, AffixRule, AffixRuleIndex, CaseLanguage, CasePattern,
    CompoundConfig, CompoundPattern, CompoundPosition, CompoundRule, CompoundSyllableLimit,
    Condition, ConditionAtom, Flag, FlagMode, FlagSet, FormState, InputConversion, Lexeme,
    Morphology, MorphologyId, MorphologyTable, SpecialFlags,
};
pub(crate) use parse::{
    apply_conversions, diagnostic, enforce_byte_input_limits, enforce_input_limit,
    is_variation_selector, normalize_affix_text_for_ignored_characters, parse_aff, parse_dic,
    BreakPattern, ParsedAff,
};

pub use ferrolex_compiler::DictionaryIr;
pub use ferrolex_suggest::{
    CandidateSource, RankingSignals, ReplacementRule, SuggestConfig, Suggester,
};

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

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(
            formatter,
            "{}:{}: {severity}[{}]: {}",
            self.source, self.line, self.directive, self.message
        )
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
        )?;
        if let Some(first) = self.diagnostics.first() {
            write!(formatter, "; first: {first}")?;
        }
        Ok(())
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

    /// Consumes the import result and returns its runtime dictionary.
    ///
    /// This moves the dictionary out without cloning its potentially large
    /// lexeme and index structures. The source-neutral IR and diagnostics are
    /// dropped.
    #[must_use]
    pub fn into_dictionary(self) -> HunspellDictionary {
        self.dictionary
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
    /// Encoding used to interpret flags in the imported AFF/DIC pair.
    flag_mode: FlagMode,
    /// Whether Hunspell capitalization fallback is enabled for this pair.
    case_fallback: bool,
    /// Language-specific casing policy used by capitalization fallback.
    case_language: CaseLanguage,
    /// Stable stem indices used when lowering the runtime dictionary to IR.
    unique_stem_indices: Vec<u32>,
    /// Interned morphology fields retained for explanations and IR lowering.
    morphology: MorphologyTable,
    /// Stored stems together with their flags and morphology references.
    lexemes: Vec<Lexeme>,
    /// Prefix rules evaluated during lazy derivation.
    prefixes: Vec<AffixRule>,
    /// Suffix rules evaluated during lazy derivation.
    suffixes: Vec<AffixRule>,
    /// Reverse lookup index for prefix additions.
    prefix_rules_by_add_edge: AffixRuleIndex,
    /// Reverse lookup index for suffix additions.
    suffix_rules_by_add_edge: AffixRuleIndex,
    /// Forward prefix rules grouped by the flag that enables them.
    prefix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    /// Forward suffix rules grouped by the flag that enables them.
    suffix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    /// Special Hunspell markers such as KEEPCASE and ONLYINCOMPOUND.
    special_flags: SpecialFlags,
    /// Compound flags, limits, patterns, and boundary safeguards.
    compound: CompoundConfig,
    /// BREAK rules used to retry recognition on bounded word fragments.
    break_patterns: Vec<BreakPattern>,
    /// Accepted CHECKSHARPS-derived uppercase spellings.
    sharp_uppercase_forms: BTreeSet<Box<str>>,
    /// WORDCHARS metadata retained for tokenization-aware consumers.
    word_characters: BTreeSet<char>,
    /// REP rules used for suggestion ranking and compound safeguards.
    replacement_rules: Vec<ReplacementRule>,
    /// KEY keyboard layout used for suggestion ranking.
    keyboard: Option<Box<str>>,
    /// MAP character substitutions used for suggestion ranking.
    character_maps: Vec<String>,
    /// IGNORE characters removed before recognition.
    ignored_characters: BTreeSet<char>,
    /// ICONV rules applied before recognition.
    input_conversions: Vec<InputConversion>,
    /// OCONV rules applied when rendering suggestion spellings.
    output_conversions: Vec<InputConversion>,
    /// Whether FULLSTRIP permits stripping an entire stem.
    full_strip: bool,
    /// Whether COMPLEXPREFIXES permits a second prefix.
    complex_prefixes: bool,
    /// Lazily initialized candidate index for bounded suggestions.
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

    fn as_candidate_source(&self) -> Option<&dyn CandidateSource> {
        Some(self)
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

    /// Runs the direct, affix, and compound recognition cascade.
    ///
    /// `allow_keep_case` is true for the caller's exact spelling and false for
    /// synthetic capitalization-fallback candidates. This preserves the
    /// `KEEPCASE` contract: an exact flagged entry may match, but lower- or
    /// initial-case fallback must not admit it. The affix and compound stages
    /// are lazy and bounded; their detailed directive contracts live in
    /// `docs/affix-semantics.md` and `docs/compound-semantics.md`.
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

    /// Creates a suggester preconfigured with this dictionary's `REP`, `KEY`,
    /// and `MAP` data.
    ///
    /// Hunspell `OCONV` rules transform suggestion output rather than the
    /// candidate source. Apply [`Self::normalize_output`] to every returned
    /// spelling before displaying or storing it.
    ///
    /// ```
    /// use ferrolex_hunspell::{import, ImportMode, SuggestConfig};
    ///
    /// let imported = import(
    ///     "example.aff",
    ///     "SET UTF-8\nREP 1\nREP teh the\nOCONV 1\nOCONV the æ\n",
    ///     "example.dic",
    ///     "1\nthe\n",
    ///     ImportMode::Strict,
    /// )?;
    /// let dictionary = imported.dictionary();
    /// let result = dictionary
    ///     .suggester(SuggestConfig {
    ///         max_edit_distance: 0,
    ///         ..SuggestConfig::default()
    ///     })
    ///     .suggest("teh");
    /// let output: Vec<_> = result
    ///     .suggestions()
    ///     .iter()
    ///     .map(|suggestion| dictionary.normalize_output(suggestion.word()))
    ///     .collect();
    /// assert_eq!(output, ["æ"]);
    /// # Ok::<(), ferrolex_hunspell::ImportError>(())
    /// ```
    #[must_use]
    pub fn suggester(&self, config: SuggestConfig) -> Suggester<'_, Self> {
        Suggester::new(self, config)
            .with_replacement_rules(self.replacement_rules())
            .with_ranking_signals(self.ranking_signals())
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
        self.extend_reverse_derived_candidates(word, &mut candidates)?;
        Some(candidates)
    }

    fn extend_reverse_derived_candidates(
        &self,
        word: &str,
        candidates: &mut BTreeSet<usize>,
    ) -> Option<()> {
        if self.prefixes.is_empty() && self.suffixes.is_empty() {
            return Some(());
        }

        let mut forms = BTreeSet::from([(word.to_owned(), 0_usize)]);
        let mut pending = vec![(word.to_owned(), 0_usize)];
        while let Some((form, depth)) = pending.pop() {
            if depth == MAX_AFFIX_CHAIN {
                continue;
            }
            for rule in self.candidate_affix_rules(&form) {
                let Some(stem) = rule.reverse_apply(&form, self.full_strip) else {
                    continue;
                };
                for index in self.lexeme_index_range(&stem) {
                    candidates.insert(index);
                    if candidates.len() > MAX_DERIVED_CANDIDATES_PER_LOOKUP {
                        return None;
                    }
                }
                let stem = stem.into_owned();
                let next_depth = depth + 1;
                if forms.insert((stem.clone(), next_depth)) {
                    if forms.len() > MAX_REVERSE_FORMS_PER_LOOKUP {
                        return None;
                    }
                    pending.push((stem, next_depth));
                }
            }
        }
        Some(())
    }

    /// Resolves one lexeme's lazily derived affix forms with bounded DFS.
    ///
    /// A [`FormState`] is one unique affix chain: `can_apply` prevents a rule
    /// from repeating, keeps prefixes before suffixes, and enforces the
    /// `COMPLEXPREFIXES`/cross-product limits. `MAX_AFFIX_CHAIN` bounds the
    /// depth and `MAX_DERIVATIONS_PER_LEXEME` bounds the number of expanded
    /// states. A false result from `expand_matching_rules` means that the
    /// derivation budget was exhausted, not that the current state simply had
    /// no matching rule, so the lookup rejects the incomplete search.
    ///
    /// This implements the lazy derivation contract in
    /// `docs/affix-semantics.md`, including `CIRCUMFIX`, `NEEDAFFIX`,
    /// `ONLYINCOMPOUND`, and `FORBIDDENWORD` acceptance checks.
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
        self.stems().any(|stem| stem == word) && self.is_suggestable_stem(word)
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
        .filter(|lexeme| {
            has_flag(&lexeme.flags, *keep_case)
                && lexeme.stem.contains('ß')
                && !special_flags
                    .forbidden_word
                    .is_some_and(|flag| has_flag(&lexeme.flags, flag))
                && !special_flags
                    .need_affix
                    .is_some_and(|flag| has_flag(&lexeme.flags, flag))
                && !special_flags
                    .only_in_compound
                    .is_some_and(|flag| has_flag(&lexeme.flags, flag))
        })
        .flat_map(|lexeme| {
            [
                Box::<str>::from(initial_case_for_language(
                    &lexeme.stem,
                    CaseLanguage::Default,
                )),
                Box::<str>::from(lexeme.stem.to_uppercase()),
            ]
        })
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::collections::BTreeSet;
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

                assert_eq!(
                    super::model::is_cased(character, language),
                    lowercase != uppercase
                );
                assert_eq!(
                    super::model::is_uppercase(character, language),
                    original != lowercase
                );
                assert_eq!(
                    super::model::is_lowercase(character, language),
                    original != uppercase
                );
            }
        }
    }

    #[test]
    fn dictionary_stem_unescaping_borrows_no_op_inputs() {
        assert!(matches!(
            super::parse::unescape_dictionary_stem("plain"),
            Cow::Borrowed("plain")
        ));
        assert_eq!(
            super::parse::unescape_dictionary_stem(r"path\/name"),
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
    fn title_case_suggestions_fall_back_to_the_stored_hunspell_stem() {
        let result = import(
            "casing.aff",
            "SET UTF-8\n",
            "casing.dic",
            "1\nNATO\n",
            ImportMode::Strict,
        )
        .expect("the casing fixture imports cleanly");

        let result = Suggester::new(result.dictionary(), SuggestConfig::default()).suggest("Nato");

        assert_eq!(result.suggestions()[0].word(), "NATO");
    }

    #[test]
    fn title_case_suggestions_do_not_use_policy_rejected_spellings() {
        let result = import(
            "policy-casing.aff",
            "SET UTF-8\nNOSUGGEST S\n",
            "policy-casing.dic",
            "2\nnato\nNato/S\n",
            ImportMode::Strict,
        )
        .expect("the policy casing fixture imports cleanly");

        let result = Suggester::new(result.dictionary(), SuggestConfig::default()).suggest("Nato");

        assert_eq!(result.suggestions()[0].word(), "nato");
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
    fn checksharps_accepts_keepcase_initial_and_ss_uppercase_forms() {
        let imported = import(
            "test.aff",
            "CHECKSHARPS\nKEEPCASE K\n",
            "test.dic",
            "2\nstraße/K\nMaße\n",
            ImportMode::Strict,
        )
        .expect("CHECKSHARPS imports");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("straße"));
        assert!(dictionary.contains("Straße"));
        assert!(dictionary.contains("STRASSE"));
        assert!(!dictionary.contains("STRAẞE"));
        assert!(!dictionary.contains("MASSE"));
    }

    #[test]
    fn checksharps_does_not_bypass_restricted_keepcase_flags() {
        let imported = import(
            "test.aff",
            "CHECKSHARPS\nKEEPCASE K\nFORBIDDENWORD F\nNEEDAFFIX N\nONLYINCOMPOUND O\n",
            "test.dic",
            "3\nstraße/KF\nbedarf/KN\nteil/KO\n",
            ImportMode::Strict,
        )
        .expect("CHECKSHARPS imports");

        let dictionary = imported.dictionary();
        for word in [
            "straße", "Straße", "STRASSE", "bedarf", "Bedarf", "BEDARF", "teil", "Teil", "TEIL",
        ] {
            assert!(
                !dictionary.contains(word),
                "restricted word `{word}` accepted"
            );
        }
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
    fn reverse_candidates_use_stems_for_non_empty_add_rules() {
        let mut dictionary = String::new();
        for index in 0..=MAX_DERIVED_CANDIDATES_PER_LOOKUP {
            writeln!(dictionary, "word{index}/A").expect("writing to String does not fail");
        }
        let dictionary = format!("{}\n{dictionary}", MAX_DERIVED_CANDIDATES_PER_LOOKUP + 1);
        let imported = import(
            "test.aff",
            "SFX A Y 1\nSFX A 0 s/B .\nSFX B Y 1\nSFX B 0 x .\n",
            "test.dic",
            &dictionary,
            ImportMode::Strict,
        )
        .expect("large non-empty affix class imports");

        let dictionary = imported.dictionary();
        let expected = dictionary
            .lexeme_index_range("word0")
            .collect::<BTreeSet<_>>();
        assert_eq!(
            dictionary
                .derived_candidate_indices("word0sx")
                .expect("reverse candidate lookup stays within its budget"),
            expected,
            "reverse lookup should find only the actual stem range"
        );
        assert_eq!(
            dictionary
                .derived_candidate_indices("not-a-generated-form-sx")
                .expect("a miss with no matching stem stays within its budget"),
            BTreeSet::new(),
            "a non-empty add must not scan its complete flag class"
        );
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
