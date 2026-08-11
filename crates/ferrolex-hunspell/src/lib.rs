//! Hunspell-compatible dictionary import for ferrolex.
//!
//! The importer accepts a deliberately documented subset of the textual
//! Hunspell format and translates it into ferrolex-owned data structures. No
//! runtime dependency on another spell checker is introduced.

#![forbid(unsafe_code)]

mod cache;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use encoding_rs::ISO_8859_2;
use ferrolex_core::Dictionary;
use ferrolex_suggest::{CandidateSource, ReplacementRule};

pub use cache::{
    compile_runtime_cache, inspect_runtime_cache, load_runtime_cache, CacheSource,
    RuntimeCacheError, RuntimeCacheMetadata, SourceDigests, HUNSPELL_CACHE_FORMAT_VERSION,
    HUNSPELL_CACHE_SEMANTICS_VERSION,
};

const MAX_AFF_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIC_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_AFFIX_RULES: usize = 100_000;
const MAX_DICTIONARY_ENTRIES: usize = 1_000_000;
const MAX_FLAGS_PER_ENTRY: usize = 256;
const MAX_CONDITION_ATOMS: usize = 256;
const MAX_AFFIX_CHAIN: usize = 8;
const MAX_DERIVATIONS_PER_LEXEME: usize = 4_096;
/// Bounds reverse-affix candidate work for one lookup. The limit is deliberately
/// lower than the import entry limit so a suffix with an empty `add` cannot turn
/// a miss into a scan of the whole dictionary.
const MAX_DERIVED_CANDIDATES_PER_LOOKUP: usize = 8_192;
const MAX_COMPOUND_SCALARS: usize = 256;
const MAX_COMPOUND_RULES: usize = 1_024;
const MAX_COMPOUND_RULE_COMPONENTS: usize = 16;
const MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE: usize = 1_024;
const MAX_COMPOUND_RULE_EXPANSIONS: usize = 16_384;
const MAX_BREAK_PATTERNS: usize = 256;
const MAX_REPLACEMENT_RULES: usize = 4_096;
const MAX_AFFIX_ALIASES: usize = 100_000;
const MAX_INPUT_CONVERSIONS: usize = 4_096;

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
    diagnostics: Vec<Diagnostic>,
}

impl ImportResult {
    /// Returns the independently represented runtime dictionary.
    #[must_use]
    pub fn dictionary(&self) -> &HunspellDictionary {
        &self.dictionary
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
    stem_indices: BTreeMap<Box<str>, Vec<usize>>,
    lexemes: Vec<Lexeme>,
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    prefix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    suffix_rules_by_flag: BTreeMap<Flag, Vec<usize>>,
    lexeme_indices_by_flag: BTreeMap<Flag, Vec<usize>>,
    prefix_parent_flags: BTreeMap<Flag, BTreeSet<Flag>>,
    suffix_parent_flags: BTreeMap<Flag, BTreeSet<Flag>>,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
    break_characters: BTreeSet<char>,
    sharp_uppercase_forms: BTreeSet<Box<str>>,
    word_characters: BTreeSet<char>,
    replacement_rules: Vec<ReplacementRule>,
    ignored_characters: BTreeSet<char>,
    input_conversions: Vec<InputConversion>,
    output_conversions: Vec<InputConversion>,
    full_strip: bool,
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
    fn contains_normalized(&self, word: &str, allow_keep_case: bool) -> bool {
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
            || self.matches_break_word(word, allow_keep_case)
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
        lexemes: Vec<Lexeme>,
        prefixes: Vec<AffixRule>,
        suffixes: Vec<AffixRule>,
        special_flags: SpecialFlags,
        compound: CompoundConfig,
        break_characters: BTreeSet<char>,
        word_characters: BTreeSet<char>,
        replacement_rules: Vec<ReplacementRule>,
        ignored_characters: BTreeSet<char>,
        input_conversions: Vec<InputConversion>,
        output_conversions: Vec<InputConversion>,
        full_strip: bool,
    ) -> Self {
        let stem_indices = stem_indices(&lexemes);
        let prefix_rules_by_flag = rule_indices_by_flag(&prefixes);
        let suffix_rules_by_flag = rule_indices_by_flag(&suffixes);
        let lexeme_indices_by_flag = lexeme_indices_by_flag(&lexemes);
        let prefix_parent_flags = parent_flags_by_continuation(&prefixes);
        let suffix_parent_flags = parent_flags_by_continuation(&suffixes);
        let sharp_uppercase_forms = sharp_uppercase_forms(&lexemes, &special_flags);
        Self {
            flag_mode,
            case_fallback,
            case_language,
            stem_indices,
            lexemes,
            prefixes,
            suffixes,
            prefix_rules_by_flag,
            suffix_rules_by_flag,
            lexeme_indices_by_flag,
            prefix_parent_flags,
            suffix_parent_flags,
            special_flags,
            compound,
            break_characters,
            sharp_uppercase_forms,
            word_characters,
            replacement_rules,
            ignored_characters,
            input_conversions,
            output_conversions,
            full_strip,
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
        self.stem_indices.keys().map(Box::as_ref)
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

    fn lexemes_for_stem(&self, stem: &str) -> impl Iterator<Item = &Lexeme> {
        self.stem_indices
            .get(stem)
            .into_iter()
            .flatten()
            .map(|index| &self.lexemes[*index])
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
        self.prefixes.iter().chain(&self.suffixes).any(|rule| {
            rule.could_generate(word)
                && rule
                    .reverse_apply(word, self.full_strip)
                    .is_some_and(|stem| {
                        self.lexemes_for_stem(&stem).any(|lexeme| {
                            !self.is_forbidden(&lexeme.flags)
                                && lexeme.flags.contains(&rule.flag)
                                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                                && self.is_accepted_state(&FormState::new(lexeme).apply(
                                    rule,
                                    word.to_owned(),
                                    &self.special_flags,
                                ))
                        })
                    })
        })
    }

    fn derived_candidate_indices(&self, word: &str) -> Option<BTreeSet<usize>> {
        let mut candidates = BTreeSet::new();
        self.extend_derived_candidates(
            word,
            &self.prefixes,
            &self.prefix_parent_flags,
            &mut candidates,
        )?;
        self.extend_derived_candidates(
            word,
            &self.suffixes,
            &self.suffix_parent_flags,
            &mut candidates,
        )?;
        Some(candidates)
    }

    fn extend_derived_candidates(
        &self,
        word: &str,
        rules: &[AffixRule],
        parent_flags: &BTreeMap<Flag, BTreeSet<Flag>>,
        candidates: &mut BTreeSet<usize>,
    ) -> Option<()> {
        for rule in rules.iter().filter(|rule| rule.could_generate(word)) {
            for flag in origin_flags_for(&rule.flag, parent_flags) {
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

    fn matches_derived_word(&self, lexeme: &Lexeme, word: &str, allow_keep_case: bool) -> bool {
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
            .is_some_and(|flag| rule.continuation_flags.contains(flag));
        match position {
            CompoundPosition::Begin => rule.kind == AffixKind::Prefix || permit,
            CompoundPosition::Middle => permit,
            CompoundPosition::End => rule.kind == AffixKind::Suffix || permit,
        }
    }

    fn expand_matching_rules(
        &self,
        state: &FormState,
        kind: AffixKind,
        rules: &[AffixRule],
        rule_indices_by_flag: &BTreeMap<Flag, Vec<usize>>,
        states: &mut Vec<FormState>,
        derivations: &mut usize,
    ) -> bool {
        let flags = state.flags_for(kind);
        for flag in flags {
            let Some(rule_indices) = rule_indices_by_flag.get(flag) else {
                continue;
            };
            for index in rule_indices {
                let rule = &rules[*index];
                if !state.can_apply(rule) {
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

    fn is_accepted_state(&self, state: &FormState) -> bool {
        !self.is_forbidden(&state.flags)
            && !self.requires_affix(&state.flags)
            && !self.is_only_in_compound(&state.origin_flags)
            && !self.is_only_in_compound(&state.flags)
            && state.has_complete_circumfix()
    }

    fn is_accepted_compound_state(&self, state: &FormState) -> bool {
        !self.is_forbidden(&state.flags)
            && !self.requires_affix(&state.flags)
            && state.has_complete_circumfix()
    }

    fn is_forbidden(&self, flags: &BTreeSet<Flag>) -> bool {
        self.special_flags
            .forbidden_word
            .as_ref()
            .is_some_and(|flag| flags.contains(flag))
    }

    fn requires_affix(&self, flags: &BTreeSet<Flag>) -> bool {
        self.special_flags
            .need_affix
            .as_ref()
            .is_some_and(|flag| flags.contains(flag))
    }

    fn is_only_in_compound(&self, flags: &BTreeSet<Flag>) -> bool {
        self.special_flags
            .only_in_compound
            .as_ref()
            .is_some_and(|flag| flags.contains(flag))
    }

    fn is_keep_case(&self, flags: &BTreeSet<Flag>) -> bool {
        self.special_flags
            .keep_case
            .as_ref()
            .is_some_and(|flag| flags.contains(flag))
    }

    fn matches_simple_compound(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.compound.flag.is_none()
            && self.compound.rules.is_empty()
            && (self.compound.begin.is_none() || self.compound.end.is_none())
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

        self.compound.flag.as_ref().is_some_and(|flag| {
            self.matches_compound_pattern(word, &boundaries, None, Some(flag), allow_keep_case)
        }) || self.compound.rules.iter().any(|rule| {
            rule.patterns.iter().any(|pattern| {
                self.matches_compound_pattern(
                    word,
                    &boundaries,
                    Some(pattern),
                    None,
                    allow_keep_case,
                )
            })
        }) || self.matches_positioned_compound(word, &boundaries, allow_keep_case)
    }

    fn matches_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: Option<&[Flag]>,
        generic_flag: Option<&Flag>,
        allow_keep_case: bool,
    ) -> bool {
        if let Some(pattern) = pattern {
            return self.matches_fixed_compound_pattern(word, boundaries, pattern, allow_keep_case);
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
                flag,
                allow_keep_case,
            );
            if component_count >= 2 && next.last() == Some(&true) {
                return true;
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
                flag,
                allow_keep_case,
            );
            if next.iter().all(|reachable| !reachable) {
                return false;
            }
            reachable = next;
        }
        reachable.last() == Some(&true)
    }

    fn extend_compound_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        flag: &Flag,
        allow_keep_case: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.matches_compound_component(candidate, flag, allow_keep_case) {
                    next[end] = true;
                }
            }
        }
        next
    }

    fn matches_compound_component(
        &self,
        word: &str,
        required_flag: &Flag,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && lexeme.flags.contains(required_flag)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
        })
    }

    fn matches_positioned_compound(
        &self,
        word: &str,
        boundaries: &[usize],
        allow_keep_case: bool,
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
            begin,
            CompoundPosition::Begin,
            allow_keep_case,
        );
        for _ in 2..boundaries.len() {
            let terminal = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                end,
                CompoundPosition::End,
                allow_keep_case,
            );
            if terminal.last() == Some(&true) {
                return true;
            }
            let Some(middle) = self.compound.middle.as_ref() else {
                return false;
            };
            reachable = self.extend_positioned_components(
                word,
                boundaries,
                &reachable,
                middle,
                CompoundPosition::Middle,
                allow_keep_case,
            );
        }
        false
    }

    fn extend_positioned_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        position_flag: &Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.matches_positioned_component(
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
        position_flag: &Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.lexemes_for_stem(word).any(|lexeme| {
            !self.is_forbidden(&lexeme.flags)
                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                && (lexeme.flags.contains(position_flag)
                    || self
                        .compound
                        .flag
                        .as_ref()
                        .is_some_and(|flag| lexeme.flags.contains(flag)))
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
        position_flag: &Flag,
        position: CompoundPosition,
        allow_keep_case: bool,
    ) -> bool {
        self.prefixes
            .iter()
            .chain(&self.suffixes)
            .filter(|rule| self.compound_rule_is_allowed(rule, position))
            .any(|rule| {
                rule.reverse_apply(word, self.full_strip)
                    .is_some_and(|stem| {
                        self.lexemes_for_stem(&stem).any(|lexeme| {
                            !self.is_forbidden(&lexeme.flags)
                                && (allow_keep_case || !self.is_keep_case(&lexeme.flags))
                                && lexeme.flags.contains(&rule.flag)
                                && (lexeme.flags.contains(position_flag)
                                    || self
                                        .compound
                                        .flag
                                        .as_ref()
                                        .is_some_and(|flag| lexeme.flags.contains(flag)))
                                && self.is_accepted_compound_state(&FormState::new(lexeme).apply(
                                    rule,
                                    word.to_owned(),
                                    &self.special_flags,
                                ))
                        })
                    })
            })
    }

    fn matches_break_word(&self, word: &str, allow_keep_case: bool) -> bool {
        if self.break_characters.is_empty() || word.chars().count() > MAX_COMPOUND_SCALARS {
            return false;
        }
        let mut parts = word.split(|character| self.break_characters.contains(&character));
        let Some(first) = parts.next() else {
            return false;
        };
        if first.is_empty() {
            return false;
        }
        let mut had_break = false;
        for part in parts {
            had_break = true;
            if part.is_empty() || !self.contains_normalized(part, allow_keep_case) {
                return false;
            }
        }
        had_break && self.contains_normalized(first, allow_keep_case)
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
        .filter(|lexeme| lexeme.flags.contains(keep_case) && lexeme.stem.contains('ß'))
        .map(|lexeme| Box::<str>::from(lexeme.stem.to_uppercase()))
        .collect()
}

fn stem_indices(lexemes: &[Lexeme]) -> BTreeMap<Box<str>, Vec<usize>> {
    let mut indices = BTreeMap::<Box<str>, Vec<usize>>::new();
    for (index, lexeme) in lexemes.iter().enumerate() {
        indices.entry(lexeme.stem.clone()).or_default().push(index);
    }
    indices
}

fn rule_indices_by_flag(rules: &[AffixRule]) -> BTreeMap<Flag, Vec<usize>> {
    let mut indices = BTreeMap::<Flag, Vec<usize>>::new();
    for (index, rule) in rules.iter().enumerate() {
        indices.entry(rule.flag.clone()).or_default().push(index);
    }
    indices
}

fn lexeme_indices_by_flag(lexemes: &[Lexeme]) -> BTreeMap<Flag, Vec<usize>> {
    let mut indices = BTreeMap::<Flag, Vec<usize>>::new();
    for (index, lexeme) in lexemes.iter().enumerate() {
        for flag in &lexeme.flags {
            indices.entry(flag.clone()).or_default().push(index);
        }
    }
    indices
}

fn parent_flags_by_continuation(rules: &[AffixRule]) -> BTreeMap<Flag, BTreeSet<Flag>> {
    let mut parents = BTreeMap::<Flag, BTreeSet<Flag>>::new();
    for rule in rules {
        for continuation in &rule.continuation_flags {
            parents
                .entry(continuation.clone())
                .or_default()
                .insert(rule.flag.clone());
        }
    }
    parents
}

fn origin_flags_for(
    terminal_flag: &Flag,
    parent_flags: &BTreeMap<Flag, BTreeSet<Flag>>,
) -> BTreeSet<Flag> {
    let mut origins = BTreeSet::from([terminal_flag.clone()]);
    let mut pending = vec![terminal_flag.clone()];
    while let Some(flag) = pending.pop() {
        if let Some(parents) = parent_flags.get(&flag) {
            for parent in parents {
                if origins.insert(parent.clone()) {
                    pending.push(parent.clone());
                }
            }
        }
    }
    origins
}

#[derive(Clone, Debug)]
struct Lexeme {
    stem: Box<str>,
    flags: BTreeSet<Flag>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Flag(Box<str>);

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
    lowercase_character(character, language) != uppercase_character(character, language)
}

fn is_uppercase(character: char, language: CaseLanguage) -> bool {
    character.to_string() != lowercase_character(character, language)
}

fn is_lowercase(character: char, language: CaseLanguage) -> bool {
    character.to_string() != uppercase_character(character, language)
}

fn lowercase_for_language(word: &str, language: CaseLanguage) -> String {
    let mut result = String::with_capacity(word.len());
    for character in word.chars() {
        result.push_str(&lowercase_character(character, language));
    }
    result
}

fn initial_case_for_language(word: &str, language: CaseLanguage) -> String {
    let Some((index, first)) = word.char_indices().next() else {
        return String::new();
    };
    let mut result = uppercase_character(first, language);
    result.push_str(&lowercase_for_language(
        &word[index + first.len_utf8()..],
        language,
    ));
    result
}

fn lowercase_character(character: char, language: CaseLanguage) -> String {
    if language == CaseLanguage::Turkic {
        match character {
            'I' => return "ı".to_owned(),
            'İ' => return "i".to_owned(),
            _ => {}
        }
    }
    character.to_lowercase().collect()
}

fn uppercase_character(character: char, language: CaseLanguage) -> String {
    if language == CaseLanguage::Turkic {
        match character {
            'i' => return "İ".to_owned(),
            'ı' => return "I".to_owned(),
            _ => {}
        }
    }
    character.to_uppercase().collect()
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

#[derive(Clone, Copy)]
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
    continuation_flags: BTreeSet<Flag>,
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

    fn reverse_apply(&self, form: &str, full_strip: bool) -> Option<String> {
        let stem = match self.kind {
            AffixKind::Prefix => {
                let remaining = form.strip_prefix(self.add.as_ref())?;
                let mut stem = String::with_capacity(self.strip.len() + remaining.len());
                stem.push_str(&self.strip);
                stem.push_str(remaining);
                stem
            }
            AffixKind::Suffix => {
                let remaining = form.strip_suffix(self.add.as_ref())?;
                let mut stem = String::with_capacity(remaining.len() + self.strip.len());
                stem.push_str(remaining);
                stem.push_str(&self.strip);
                stem
            }
        };
        (self.apply(&stem, full_strip).as_deref() == Some(form)).then_some(stem)
    }
}

#[derive(Clone, Debug)]
struct FormState {
    form: String,
    flags: BTreeSet<Flag>,
    origin_flags: BTreeSet<Flag>,
    depth: usize,
    prefix_count: usize,
    suffix_count: usize,
    last_kind: Option<AffixKind>,
    last_cross_product: bool,
    used_rules: BTreeSet<usize>,
    circumfix_prefix: bool,
    circumfix_suffix: bool,
}

impl FormState {
    fn new(lexeme: &Lexeme) -> Self {
        Self {
            form: lexeme.stem.to_string(),
            flags: lexeme.flags.clone(),
            origin_flags: lexeme.flags.clone(),
            depth: 0,
            prefix_count: 0,
            suffix_count: 0,
            last_kind: None,
            last_cross_product: true,
            used_rules: BTreeSet::new(),
            circumfix_prefix: false,
            circumfix_suffix: false,
        }
    }

    fn can_apply(&self, rule: &AffixRule) -> bool {
        !self.used_rules.contains(&rule.id)
            && match rule.kind {
                // Hunspell permits one prefix. A prefix is applied before any
                // suffix so the resulting form remains unambiguous.
                AffixKind::Prefix => self.prefix_count == 0 && self.suffix_count == 0,
                // Continuation classes may supply one additional suffix.
                AffixKind::Suffix => self.suffix_count < 2,
            }
            && match self.last_kind {
                None => self.flags.contains(&rule.flag),
                Some(kind) if kind == rule.kind => self.flags.contains(&rule.flag),
                Some(_) => {
                    self.last_cross_product
                        && rule.cross_product
                        && self.origin_flags.contains(&rule.flag)
                }
            }
    }

    fn flags_for(&self, kind: AffixKind) -> &BTreeSet<Flag> {
        match self.last_kind {
            Some(previous_kind) if previous_kind != kind => &self.origin_flags,
            Some(_) | None => &self.flags,
        }
    }

    fn apply(&self, rule: &AffixRule, form: String, special_flags: &SpecialFlags) -> Self {
        let circumfix = special_flags
            .circumfix
            .as_ref()
            .is_some_and(|flag| rule.continuation_flags.contains(flag));
        let mut used_rules = self.used_rules.clone();
        used_rules.insert(rule.id);
        Self {
            form,
            flags: rule.continuation_flags.clone(),
            origin_flags: self.origin_flags.clone(),
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
    check_sharps: bool,
}

#[derive(Clone, Debug)]
struct CompoundConfig {
    flag: Option<Flag>,
    begin: Option<Flag>,
    middle: Option<Flag>,
    end: Option<Flag>,
    permit: Option<Flag>,
    minimum_length: usize,
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
            minimum_length: 3,
            rules: Vec::new(),
        }
    }
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
        let characters = stem.chars().collect::<Vec<_>>();
        if characters.len() < self.atoms.len() {
            return false;
        }
        let start = match kind {
            AffixKind::Prefix => 0,
            AffixKind::Suffix => characters.len() - self.atoms.len(),
        };
        let start = if self.anchored_at_start { 0 } else { start };

        self.atoms
            .iter()
            .zip(&characters[start..])
            .all(|(atom, character)| atom.matches(*character))
            && self.not_preceded_by.as_ref().map_or(true, |atom| {
                start == 0 || !atom.matches(characters[start - 1])
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
/// encoding. This prevents an override from silently interpreting a pair with
/// an incompatible declared format. Use this only when a source catalog
/// establishes a dictionary-file exception to the normal shared encoding.
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
    if declared != encodings.aff() {
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
            parsed_aff.morphology_aliases.len(),
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
        lexemes,
        parsed_aff.prefixes,
        parsed_aff.suffixes,
        parsed_aff.special_flags,
        parsed_aff.compound,
        parsed_aff.break_characters,
        parsed_aff.word_characters,
        parsed_aff.replacement_rules,
        parsed_aff.ignored_characters,
        parsed_aff.input_conversions,
        parsed_aff.output_conversions,
        parsed_aff.full_strip,
    );

    if mode == ImportMode::Strict
        && diagnostics
            .iter()
            .any(|item| item.severity == Severity::Error)
    {
        return Err(ImportError { diagnostics });
    }

    Ok(ImportResult {
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
    match encoding {
        ByteEncoding::Utf8 => match std::str::from_utf8(bytes) {
            Ok(text) => {
                if strip_utf8_bom {
                    text.strip_prefix('\u{feff}').unwrap_or(text).to_owned()
                } else {
                    text.to_owned()
                }
            }
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
    }
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
    break_characters: BTreeSet<char>,
    word_characters: BTreeSet<char>,
    replacement_rules: Vec<ReplacementRule>,
    flag_aliases: Vec<Option<BTreeSet<Flag>>>,
    morphology_aliases: Vec<Option<Box<str>>>,
    ignored_characters: BTreeSet<char>,
    input_conversions: Vec<InputConversion>,
    output_conversions: Vec<InputConversion>,
    full_strip: bool,
    declared_sections: BTreeSet<CountedSection>,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum CountedSection {
    ReplacementRules,
    FlagAliases,
    MorphologyAliases,
    InputConversions,
    OutputConversions,
    BreakPatterns,
}

#[allow(
    clippy::too_many_lines,
    reason = "the directive dispatch stays together to preserve the line-oriented parser contract"
)]
fn parse_aff(source: &str, text: &str) -> ParsedAff {
    let mut parsed = ParsedAff {
        break_characters: BTreeSet::from(['-']),
        ..ParsedAff::default()
    };
    let mut lines = text.lines().enumerate();

    while let Some((index, original_line)) = lines.next() {
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
                "line exceeds the configured 16 KiB importer limit",
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
                &mut parsed.full_strip,
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
            "COMPOUNDMIN" => parse_compound_minimum(
                source,
                line_number,
                &fields,
                &mut parsed.compound,
                &mut parsed.diagnostics,
            ),
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
        let Some((index, line)) = next_alias_line(lines) else {
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
            ["AF"] => Some(BTreeSet::new()),
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
        let Some((index, line)) = next_alias_line(lines) else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "AM",
                Severity::Warning,
                "AM header ended before all declared aliases were supplied",
            ));
            return;
        };
        let alias = line
            .strip_prefix("AM")
            .map(str::trim_start)
            .filter(|alias| !alias.is_empty())
            .map(Box::from);
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
        let Some((index, line)) = next_alias_line(lines) else {
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
        let Some((index, line)) = next_alias_line(lines) else {
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

fn next_alias_line<'source>(
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
        rule.strip = remove_ignored_characters(&rule.strip, &parsed.ignored_characters);
        rule.add = remove_ignored_characters(&rule.add, &parsed.ignored_characters);
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

fn remove_ignored_characters(value: &str, ignored_characters: &BTreeSet<char>) -> Box<str> {
    Box::from(
        value
            .chars()
            .filter(|character| !ignored_characters.contains(character))
            .collect::<String>(),
    )
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
        let Some((index, line)) = lines.next() else {
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
        if minimum_length == 0 {
            diagnostics.push(diagnostic(
                source,
                line,
                "COMPOUNDMIN",
                Severity::Error,
                "COMPOUNDMIN must be greater than zero",
            ));
        } else {
            compound.minimum_length = minimum_length;
        }
    } else {
        diagnostics.push(diagnostic(
            source,
            line,
            "COMPOUNDMIN",
            Severity::Error,
            "COMPOUNDMIN requires a positive integer",
        ));
    }
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
        let Some((index, line)) = lines.next() else {
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
        return Err("parenthesized COMPOUNDRULE groups are not supported");
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
        parts.push((Flag(Box::from(token)), minimum, maximum));
    }
    let mut patterns = vec![Vec::new()];
    for (flag, minimum, maximum) in parts {
        let mut expanded = Vec::new();
        for prefix in patterns {
            for count in minimum..=maximum.min(MAX_COMPOUND_RULE_COMPONENTS - prefix.len()) {
                let mut next = prefix.clone();
                next.extend(std::iter::repeat(flag.clone()).take(count));
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
    if fields.len() != 2 || pattern_count == 0 || pattern_count > MAX_BREAK_PATTERNS {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            "BREAK",
            Severity::Error,
            "BREAK count must be between 1 and 256",
        ));
        return;
    }
    if !parsed
        .declared_sections
        .contains(&CountedSection::BreakPatterns)
    {
        // Hunspell's default BREAK set is `-`, `^-`, and `-$`. The latter two
        // only reject a leading or trailing separator, which the component
        // splitter already does, so the stored literal `-` preserves the
        // supported recognition behavior.
        parsed.break_characters.clear();
        parsed
            .declared_sections
            .insert(CountedSection::BreakPatterns);
    }
    for _ in 0..pattern_count {
        let Some((index, line)) = lines.next() else {
            parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                "BREAK",
                Severity::Error,
                "BREAK header ended before all declared patterns were supplied",
            ));
            return;
        };
        let rule_fields = line.split_whitespace().collect::<Vec<_>>();
        let pattern = rule_fields.get(1).copied().unwrap_or_default();
        let mut characters = pattern.chars();
        let character = characters.next();
        if rule_fields.len() != 2
            || rule_fields[0] != "BREAK"
            || character.is_none()
            || characters.next().is_some()
        {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "BREAK",
                Severity::Error,
                "only one literal Unicode-scalar break character is supported",
            ));
            continue;
        }
        parsed
            .break_characters
            .insert(character.expect("checked above"));
    }
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
                "line exceeds the configured 16 KiB importer limit",
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
            &flag,
            cross_product,
            parsed.flag_mode,
            &parsed.flag_aliases,
            rule_line,
        ) {
            Ok(rule) => {
                let rule_fields = aff_fields(rule_line);
                if rule_fields.len() > 5 {
                    parsed.diagnostics.push(diagnostic(
                        source,
                        rule_line_number,
                        directive,
                        Severity::Warning,
                        "affix morphology fields are not implemented and are ignored",
                    ));
                }
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

fn parse_affix_rule(
    id: usize,
    expected_directive: &str,
    header_flag: &Flag,
    cross_product: bool,
    flag_mode: FlagMode,
    flag_aliases: &[Option<BTreeSet<Flag>>],
    line: &str,
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
    if &rule_flag != header_flag {
        return Err("affix rule flag does not match its header".to_owned());
    }
    let (add, continuation_flags) = match fields[3].split_once('/') {
        None => (fields[3], BTreeSet::new()),
        Some((_, "")) => return Err("affix continuation flags must not be empty".to_owned()),
        Some((_, flags))
            if !is_flag_alias_reference(flags, flag_aliases)
                && !flag_mode
                    .flag_count(flags)
                    .is_some_and(|count| count <= MAX_FLAGS_PER_ENTRY) =>
        {
            return Err("affix continuation flags exceed the 256-flag importer limit".to_owned())
        }
        Some((add, flags)) => decode_entry_flags(flags, flag_mode, flag_aliases)
            .map(|flags| (add, flags))
            .ok_or_else(|| "affix continuation flags are invalid".to_owned())?,
    };
    let condition = parse_condition(fields.get(4).copied().unwrap_or("."))?;
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

#[allow(clippy::too_many_lines)]
fn parse_dic(
    source: &str,
    text: &str,
    flag_mode: FlagMode,
    flag_aliases: &[Option<BTreeSet<Flag>>],
    morphology_alias_count: usize,
    ignored_characters: &BTreeSet<char>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Lexeme> {
    let mut entries = Vec::new();
    let mut expected_count = None;
    let mut first_content = true;
    let mut entry_count = 0;

    for (index, original_line) in text.lines().enumerate() {
        let line = original_line.trim();
        if is_ignored_line(line) {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            diagnostics.push(diagnostic(
                source,
                index + 1,
                "entry",
                Severity::Error,
                "line exceeds the configured 16 KiB importer limit",
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
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let field = fields.first().copied().unwrap_or_default();
        let (stem, flags) = field
            .split_once('/')
            .map_or((field, None), |(stem, flags)| (stem, Some(flags)));
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
            None => BTreeSet::new(),
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
            Some(value)
                if !is_flag_alias_reference(value, flag_aliases)
                    && !flag_mode
                        .flag_count(value)
                        .is_some_and(|count| count <= MAX_FLAGS_PER_ENTRY) =>
            {
                diagnostics.push(diagnostic(
                    source,
                    index + 1,
                    "entry",
                    Severity::Error,
                    "dictionary entry exceeds the 256-flag importer limit",
                ));
                continue;
            }
            Some(value) => {
                if let Some(flags) = decode_entry_flags(value, flag_mode, flag_aliases) {
                    flags
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
        validate_morphology_alias_reference(
            source,
            index + 1,
            fields.get(1).copied(),
            morphology_alias_count,
            diagnostics,
        );
        entries.push(Lexeme {
            stem,
            flags: entry_flags,
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

fn decode_entry_flags(
    value: &str,
    flag_mode: FlagMode,
    aliases: &[Option<BTreeSet<Flag>>],
) -> Option<BTreeSet<Flag>> {
    if is_flag_alias_reference(value, aliases) {
        let alias = value.parse::<usize>().ok()?.checked_sub(1)?;
        aliases.get(alias)?.clone()
    } else {
        decode_flags(value, flag_mode)
    }
}

fn is_flag_alias_reference(value: &str, aliases: &[Option<BTreeSet<Flag>>]) -> bool {
    !aliases.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_morphology_alias_reference(
    source: &str,
    line: usize,
    value: Option<&str>,
    alias_count: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if alias_count == 0 {
        return;
    }
    let Some(value) = value else {
        return;
    };
    let Some(alias) = value
        .parse::<usize>()
        .ok()
        .and_then(|value| value.checked_sub(1))
    else {
        return;
    };
    if alias >= alias_count {
        diagnostics.push(diagnostic(
            source,
            line,
            "AM",
            Severity::Warning,
            "dictionary entry references an undefined AM morphology alias",
        ));
    }
}

fn decode_flags(value: &str, flag_mode: FlagMode) -> Option<BTreeSet<Flag>> {
    decode_flag_sequence(value, flag_mode).map(|flags| flags.into_iter().collect())
}

fn decode_flag_sequence(value: &str, flag_mode: FlagMode) -> Option<Vec<Flag>> {
    if flag_mode == FlagMode::Numeric {
        return value
            .split(',')
            .map(|flag| {
                flag.parse::<u32>()
                    .ok()
                    .filter(|flag| *flag > 0)
                    .map(|flag| Flag(Box::<str>::from(flag.to_string())))
            })
            .collect();
    }
    if flag_mode == FlagMode::Unicode {
        return unicode_flag_tokens(value).map(|tokens| {
            tokens
                .into_iter()
                .map(|token| Flag(Box::from(token)))
                .collect()
        });
    }
    let characters = value.chars().collect::<Vec<_>>();
    (!characters.is_empty() && characters.len() % 2 == 0).then(|| {
        characters
            .chunks_exact(2)
            .map(|chunk| Flag(Box::<str>::from(chunk.iter().collect::<String>())))
            .collect()
    })
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
            return (!value.is_empty()
                && value
                    .split(',')
                    .all(|flag| flag.parse::<u32>().is_ok_and(|flag| flag > 0)))
            .then(|| value.split(',').count());
        }
        if self == Self::Unicode {
            return unicode_flag_tokens(value).map(|tokens| tokens.len());
        }
        let count = value.chars().count();
        (count % 2 == 0).then_some(count / 2)
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
        "KEY"
            | "MAP"
            | "MAXCPDSUGS"
            | "MAXDIFF"
            | "MAXNGRAMSUGS"
            | "NGRAMSUGS"
            | "NOSPLITSUGS"
            | "NOSUGGEST"
            | "PHONE"
            | "SUGSWITHDOTS"
            | "TRY"
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
    use std::fmt::Write as _;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::Arc;
    use std::thread;

    use ferrolex_core::Dictionary;
    use ferrolex_suggest::{CandidateSource, SuggestConfig, Suggester};

    use super::{
        import, import_bytes, import_bytes_with_encodings, ByteEncoding, ByteImportEncodings,
        ImportMode, Severity, MAX_AFF_BYTES, MAX_COMPOUND_SCALARS,
        MAX_DERIVED_CANDIDATES_PER_LOOKUP, MAX_DIC_BYTES,
    };

    const AFFIXES: &str =
        "SET UTF-8\nFLAG UTF-8\nPFX A Y 1\nPFX A 0 un .\nSFX B Y 1\nSFX B y ies [^aeiou]y\n";

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
    fn resolves_af_aliases_and_validates_am_references() {
        let result = import(
            "aliases.aff",
            "AF 2\nAF AB\nAF C\nAM 2\nAM st:root\nAM st:other\nSFX B Y 1\nSFX B 0 s .\nSFX C Y 1\nSFX C 0 ed .\n",
            "aliases.dic",
            "2\nroot/1 1\nother/2 2\n",
            ImportMode::Strict,
        )
        .expect("valid aliases import cleanly");

        assert!(result.dictionary().contains("roots"));
        assert!(result.dictionary().contains("othered"));
        assert!(result.diagnostics().is_empty());
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
    fn recognition_affecting_unknown_directives_are_errors_in_strict_mode() {
        let affixes = "COMPLEXPREFIXES\n";
        let lenient = import(
            "test.aff",
            affixes,
            "test.dic",
            "1\nword\n",
            ImportMode::Lenient,
        )
        .expect("lenient import returns a safe subset");

        assert_eq!(lenient.diagnostics()[0].severity(), Severity::Error);
        assert!(import(
            "test.aff",
            affixes,
            "test.dic",
            "1\nword\n",
            ImportMode::Strict
        )
        .is_err());
    }

    #[test]
    fn resource_limits_produce_diagnostics_without_panicking() {
        let excessive_flags = "A".repeat(257);
        let dictionary = format!("1\nword/{excessive_flags}\n");
        let result = import("test.aff", "", "test.dic", &dictionary, ImportMode::Lenient)
            .expect("lenient import returns diagnostics");

        assert!(result
            .diagnostics()
            .iter()
            .any(|item| item.message().contains("256-flag importer limit")));
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
        let excessive_flags = "A".repeat(257);
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
    fn literal_break_characters_join_recognized_components() {
        let imported = import(
            "test.aff",
            "BREAK 2\nBREAK -\nBREAK .\n",
            "test.dic",
            "3\nE\nMail\nAdresse\n",
            ImportMode::Strict,
        )
        .expect("literal breaks import");

        let dictionary = imported.dictionary();
        assert!(dictionary.contains("E-Mail.Adresse"));
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
    fn complex_break_patterns_remain_explicit_strict_errors() {
        let error = import(
            "test.aff",
            "BREAK 1\nBREAK ^-\n",
            "test.dic",
            "1\nWort\n",
            ImportMode::Strict,
        )
        .expect_err("complex BREAK patterns are not approximated");

        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "BREAK"));
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
        assert!(!imported.dictionary().contains("-Mail"));
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
    fn parenthesized_compound_rules_are_precise_strict_errors() {
        for affixes in [
            "FLAG UTF-8\nCOMPOUNDRULE 1\nCOMPOUNDRULE (A)(B)\n",
            "FLAG long\nCOMPOUNDRULE 1\nCOMPOUNDRULE (aa)(bb)\n",
            "FLAG num\nCOMPOUNDRULE 1\nCOMPOUNDRULE (1)(2)\n",
        ] {
            let error = import(
                "test.aff",
                affixes,
                "test.dic",
                "1\nword\n",
                ImportMode::Strict,
            )
            .expect_err("unsupported groups must not silently change flag meaning");
            assert!(error.diagnostics().iter().any(|diagnostic| {
                diagnostic.directive() == "COMPOUNDRULE"
                    && diagnostic.message().contains("parenthesized")
            }));
        }
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

        assert!(!imported.dictionary().contains("not-a-generated-form"));
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
    fn applies_continuation_flags_and_reports_only_morphology_fields() {
        let result = import(
            "test.aff",
            "SFX A N 1\nSFX A 0 s/B . DS:plural\n",
            "test.dic",
            "1\nword/A\n",
            ImportMode::Strict,
        )
        .expect("unsupported additive metadata is only a warning");

        assert!(result.dictionary().contains("words"));
        assert!(result
            .diagnostics()
            .iter()
            .any(|item| item.message().contains("morphology fields")));
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
