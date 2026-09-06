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
mod tests;
