//! Hunspell-compatible dictionary import for ferrolex.
//!
//! The importer accepts a deliberately documented subset of the textual
//! Hunspell format and translates it into ferrolex-owned data structures. No
//! runtime dependency on another spell checker is introduced.

#![forbid(unsafe_code)]

mod cache;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use encoding_rs::ISO_8859_2;
use ferrolex_core::Dictionary;

pub use cache::{
    compile_runtime_cache, load_runtime_cache, CacheSource, RuntimeCacheError, SourceDigests,
    HUNSPELL_CACHE_FORMAT_VERSION, HUNSPELL_CACHE_SEMANTICS_VERSION,
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
const MAX_COMPOUND_SCALARS: usize = 256;
const MAX_COMPOUND_RULES: usize = 1_024;
const MAX_COMPOUND_RULE_COMPONENTS: usize = 16;
const MAX_BREAK_PATTERNS: usize = 256;

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
    stems: BTreeMap<Box<str>, BTreeSet<Flag>>,
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
}

impl Dictionary for HunspellDictionary {
    fn contains(&self, word: &str) -> bool {
        self.stems.get(word).is_some_and(|flags| {
            !self.is_forbidden(flags)
                && !self.requires_affix(flags)
                && !self.is_only_in_compound(flags)
        }) || self
            .derived_candidate_indices(word)
            .into_iter()
            .any(|index| self.matches_derived_word(&self.lexemes[index], word))
            || self.matches_simple_compound(word)
            || self.matches_break_word(word)
    }
}

impl HunspellDictionary {
    fn from_parts(
        stems: BTreeMap<Box<str>, BTreeSet<Flag>>,
        lexemes: Vec<Lexeme>,
        prefixes: Vec<AffixRule>,
        suffixes: Vec<AffixRule>,
        special_flags: SpecialFlags,
        compound: CompoundConfig,
        break_characters: BTreeSet<char>,
    ) -> Self {
        let prefix_rules_by_flag = rule_indices_by_flag(&prefixes);
        let suffix_rules_by_flag = rule_indices_by_flag(&suffixes);
        let lexeme_indices_by_flag = lexeme_indices_by_flag(&lexemes);
        let prefix_parent_flags = parent_flags_by_continuation(&prefixes);
        let suffix_parent_flags = parent_flags_by_continuation(&suffixes);
        Self {
            stems,
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
        }
    }

    fn derived_candidate_indices(&self, word: &str) -> BTreeSet<usize> {
        let mut candidates = BTreeSet::new();
        self.extend_derived_candidates(
            word,
            &self.prefixes,
            &self.prefix_parent_flags,
            &mut candidates,
        );
        self.extend_derived_candidates(
            word,
            &self.suffixes,
            &self.suffix_parent_flags,
            &mut candidates,
        );
        candidates
    }

    fn extend_derived_candidates(
        &self,
        word: &str,
        rules: &[AffixRule],
        parent_flags: &BTreeMap<Flag, BTreeSet<Flag>>,
        candidates: &mut BTreeSet<usize>,
    ) {
        for rule in rules.iter().filter(|rule| rule.could_generate(word)) {
            for flag in origin_flags_for(&rule.flag, parent_flags) {
                if let Some(indices) = self.lexeme_indices_by_flag.get(&flag) {
                    candidates.extend(indices);
                }
            }
        }
    }

    fn matches_derived_word(&self, lexeme: &Lexeme, word: &str) -> bool {
        if self.is_forbidden(&lexeme.flags) {
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
                if let Some(form) = rule.apply(&state.form) {
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
            && (!self.requires_affix(&state.origin_flags) || state.depth > 0)
            && !self.is_only_in_compound(&state.origin_flags)
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

    fn matches_simple_compound(&self, word: &str) -> bool {
        if self.compound.flag.is_none()
            && self.compound.rules.is_empty()
            && (self.compound.begin.is_none() || self.compound.end.is_none())
        {
            return false;
        };
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

        self.compound
            .flag
            .as_ref()
            .is_some_and(|flag| self.matches_compound_pattern(word, &boundaries, None, Some(flag)))
            || self.compound.rules.iter().any(|rule| {
                self.matches_compound_pattern(word, &boundaries, Some(&rule.flags), None)
            })
            || self.matches_positioned_compound(word, &boundaries)
    }

    fn matches_compound_pattern(
        &self,
        word: &str,
        boundaries: &[usize],
        pattern: Option<&[Flag]>,
        generic_flag: Option<&Flag>,
    ) -> bool {
        if let Some(pattern) = pattern {
            return self.matches_fixed_compound_pattern(word, boundaries, pattern);
        }
        let Some(flag) = generic_flag else {
            return false;
        };

        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for component_count in 1..boundaries.len() {
            let next = self.extend_compound_components(word, boundaries, &reachable, flag);
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
    ) -> bool {
        if pattern.len() < 2 {
            return false;
        }
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        for flag in pattern {
            let next = self.extend_compound_components(word, boundaries, &reachable, flag);
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
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.matches_compound_component(candidate, flag) {
                    next[end] = true;
                }
            }
        }
        next
    }

    fn matches_compound_component(&self, word: &str, required_flag: &Flag) -> bool {
        self.stems
            .get(word)
            .is_some_and(|flags| !self.is_forbidden(flags) && flags.contains(required_flag))
    }

    fn matches_positioned_compound(&self, word: &str, boundaries: &[usize]) -> bool {
        let (Some(begin), Some(end)) = (&self.compound.begin, &self.compound.end) else {
            return false;
        };
        let mut reachable = vec![false; boundaries.len()];
        reachable[0] = true;
        reachable = self.extend_positioned_components(word, boundaries, &reachable, begin);
        for _ in 2..boundaries.len() {
            let terminal = self.extend_positioned_components(word, boundaries, &reachable, end);
            if terminal.last() == Some(&true) {
                return true;
            }
            let Some(middle) = self.compound.middle.as_ref() else {
                return false;
            };
            reachable = self.extend_positioned_components(word, boundaries, &reachable, middle);
        }
        false
    }

    fn extend_positioned_components(
        &self,
        word: &str,
        boundaries: &[usize],
        reachable: &[bool],
        position_flag: &Flag,
    ) -> Vec<bool> {
        let mut next = vec![false; boundaries.len()];
        for start in 0..boundaries.len().saturating_sub(1) {
            if !reachable[start] {
                continue;
            }
            let first_end = start.saturating_add(self.compound.minimum_length);
            for end in first_end..boundaries.len() {
                let candidate = &word[boundaries[start]..boundaries[end]];
                if self.matches_positioned_component(candidate, position_flag) {
                    next[end] = true;
                }
            }
        }
        next
    }

    fn matches_positioned_component(&self, word: &str, position_flag: &Flag) -> bool {
        self.stems.get(word).is_some_and(|flags| {
            !self.is_forbidden(flags)
                && (flags.contains(position_flag)
                    || self
                        .compound
                        .flag
                        .as_ref()
                        .is_some_and(|flag| flags.contains(flag)))
        })
    }

    fn matches_break_word(&self, word: &str) -> bool {
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
            if part.is_empty() || !self.contains(part) {
                return false;
            }
        }
        had_break && self.contains(first)
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AffixKind {
    Prefix,
    Suffix,
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

    fn apply(&self, stem: &str) -> Option<String> {
        if !self.condition.matches(stem, self.kind) {
            return None;
        }

        match self.kind {
            AffixKind::Prefix => stem.strip_prefix(self.strip.as_ref()).map(|remaining| {
                let mut form = String::with_capacity(self.add.len() + remaining.len());
                form.push_str(&self.add);
                form.push_str(remaining);
                form
            }),
            AffixKind::Suffix => stem.strip_suffix(self.strip.as_ref()).map(|remaining| {
                let mut form = String::with_capacity(remaining.len() + self.add.len());
                form.push_str(remaining);
                form.push_str(&self.add);
                form
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct FormState {
    form: String,
    flags: BTreeSet<Flag>,
    origin_flags: BTreeSet<Flag>,
    depth: usize,
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
            last_kind: None,
            last_cross_product: true,
            used_rules: BTreeSet::new(),
            circumfix_prefix: false,
            circumfix_suffix: false,
        }
    }

    fn can_apply(&self, rule: &AffixRule) -> bool {
        !self.used_rules.contains(&rule.id)
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
}

#[derive(Clone, Debug)]
struct CompoundConfig {
    flag: Option<Flag>,
    begin: Option<Flag>,
    middle: Option<Flag>,
    end: Option<Flag>,
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
            minimum_length: 3,
            rules: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct CompoundRule {
    flags: Vec<Flag>,
}

#[derive(Clone, Debug)]
struct Condition {
    atoms: Vec<ConditionAtom>,
}

impl Condition {
    fn empty() -> Self {
        Self { atoms: Vec::new() }
    }

    fn matches(&self, stem: &str, kind: AffixKind) -> bool {
        if self.atoms.is_empty() {
            return true;
        }

        let characters = stem.chars().collect::<Vec<_>>();
        if characters.len() < self.atoms.len() {
            return false;
        }
        let start = match kind {
            AffixKind::Prefix => 0,
            AffixKind::Suffix => characters.len() - self.atoms.len(),
        };

        self.atoms
            .iter()
            .zip(&characters[start..])
            .all(|(atom, character)| atom.matches(*character))
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
    let parsed_aff = if enforce_input_limit(aff_source, aff_text, MAX_AFF_BYTES, &mut diagnostics) {
        parse_aff(aff_source, aff_text)
    } else {
        ParsedAff::default()
    };
    diagnostics.extend(parsed_aff.diagnostics.clone());
    let lexemes = if enforce_input_limit(dic_source, dic_text, MAX_DIC_BYTES, &mut diagnostics) {
        parse_dic(dic_source, dic_text, &mut diagnostics)
    } else {
        Vec::new()
    };
    let stems = lexemes
        .iter()
        .map(|lexeme| (lexeme.stem.clone(), lexeme.flags.clone()))
        .collect();
    let dictionary = HunspellDictionary::from_parts(
        stems,
        lexemes,
        parsed_aff.prefixes,
        parsed_aff.suffixes,
        parsed_aff.special_flags,
        parsed_aff.compound,
        parsed_aff.break_characters,
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
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    diagnostics: Vec<Diagnostic>,
    rule_count: usize,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
    break_characters: BTreeSet<char>,
}

#[allow(
    clippy::too_many_lines,
    reason = "the directive dispatch stays together to preserve the line-oriented parser contract"
)]
fn parse_aff(source: &str, text: &str) -> ParsedAff {
    let mut parsed = ParsedAff::default();
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

        let fields = line.split_whitespace().collect::<Vec<_>>();
        let directive = fields[0];
        match directive {
            "SET" => parse_set(source, line_number, &fields, &mut parsed.diagnostics),
            "FLAG" => parse_flag_mode(source, line_number, &fields, &mut parsed.diagnostics),
            "CIRCUMFIX" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.circumfix,
                &mut parsed.diagnostics,
            ),
            "FORBIDDENWORD" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.forbidden_word,
                &mut parsed.diagnostics,
            ),
            "KEEPCASE" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.keep_case,
                &mut parsed.diagnostics,
            ),
            "NEEDAFFIX" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.need_affix,
                &mut parsed.diagnostics,
            ),
            "ONLYINCOMPOUND" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.special_flags.only_in_compound,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDFLAG" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.flag,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDBEGIN" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.begin,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDMIDDLE" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.middle,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDEND" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.end,
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

fn parse_flag_mode(source: &str, line: usize, fields: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    if fields.len() != 2 {
        diagnostics.push(diagnostic(
            source,
            line,
            "FLAG",
            Severity::Error,
            "FLAG requires exactly one mode",
        ));
    } else if !matches!(fields[1].to_ascii_uppercase().as_str(), "UTF-8" | "UTF8") {
        diagnostics.push(diagnostic(
            source,
            line,
            "FLAG",
            Severity::Error,
            "only single-Unicode-scalar flags are supported in the current compatibility level",
        ));
    }
}

fn parse_special_flag(
    source: &str,
    line: usize,
    directive: &str,
    fields: &[&str],
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
    } else if let Some(flag) = decode_flag(fields[1]) {
        *target = Some(flag);
    } else {
        diagnostics.push(diagnostic(
            source,
            line,
            directive,
            Severity::Error,
            "directive flag must contain exactly one Unicode scalar",
        ));
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
        let rule_fields = line.split_whitespace().collect::<Vec<_>>();
        let pattern = rule_fields.get(1).copied().unwrap_or_default();
        let flags = pattern.chars().collect::<Vec<_>>();
        if rule_fields.len() != 2
            || rule_fields[0] != "COMPOUNDRULE"
            || flags.iter().any(|flag| matches!(flag, '*' | '+' | '?'))
            || !(2..=MAX_COMPOUND_RULE_COMPONENTS).contains(&flags.len())
        {
            parsed.diagnostics.push(diagnostic(
                source,
                index + 1,
                "COMPOUNDRULE",
                Severity::Error,
                "only 2–16 literal single-scalar component flags are supported",
            ));
            continue;
        }
        parsed.compound.rules.push(CompoundRule {
            flags: flags
                .into_iter()
                .map(|flag| Flag(Box::<str>::from(flag.to_string())))
                .collect(),
        });
    }
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
    let Some(flag) = decode_flag(fields[1]) else {
        parsed.diagnostics.push(diagnostic(
            source,
            line_number,
            directive,
            Severity::Error,
            "affix flags must contain exactly one Unicode scalar",
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
            rule_line,
        ) {
            Ok(rule) => {
                let rule_fields = rule_line.split_whitespace().collect::<Vec<_>>();
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
    line: &str,
) -> Result<AffixRule, String> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 5 {
        return Err("affix rule requires a directive, flag, strip, add, and condition".to_owned());
    }
    if fields[0] != expected_directive {
        return Err("affix rule does not match its header directive".to_owned());
    }
    let Some(rule_flag) = decode_flag(fields[1]) else {
        return Err("affix rule flag must contain exactly one Unicode scalar".to_owned());
    };
    if &rule_flag != header_flag {
        return Err("affix rule flag does not match its header".to_owned());
    }
    let (add, continuation_flags) = match fields[3].split_once('/') {
        None => (fields[3], BTreeSet::new()),
        Some((_, "")) => return Err("affix continuation flags must not be empty".to_owned()),
        Some((_, flags)) if flags.chars().count() > MAX_FLAGS_PER_ENTRY => {
            return Err("affix continuation flags exceed the 256-flag importer limit".to_owned())
        }
        Some((add, flags)) => decode_flags(flags)
            .map(|flags| (add, flags))
            .ok_or_else(|| "affix continuation flags are invalid".to_owned())?,
    };
    let condition = parse_condition(fields[4])?;
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

    let characters = field.chars().collect::<Vec<_>>();
    if characters.len() > MAX_CONDITION_ATOMS {
        return Err("condition exceeds the configured 256-atom importer limit".to_owned());
    }
    let mut atoms = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let Some(&current) = characters.get(index) else {
            break;
        };
        match current {
            '.' => {
                atoms.push(ConditionAtom::Any);
                index += 1;
            }
            '[' => {
                let Some(end) = characters[index + 1..]
                    .iter()
                    .position(|character| *character == ']')
                else {
                    return Err("condition has an unterminated bracket class".to_owned());
                };
                let end = index + 1 + end;
                let (negated, member_start) = if characters.get(index + 1) == Some(&'^') {
                    (true, index + 2)
                } else {
                    (false, index + 1)
                };
                if member_start == end {
                    return Err("condition has an empty bracket class".to_owned());
                }
                atoms.push(ConditionAtom::Class {
                    members: characters[member_start..end].iter().copied().collect(),
                    negated,
                });
                index = end + 1;
            }
            ']' | '*' | '?' | '\\' => {
                return Err("condition uses syntax outside the supported subset".to_owned())
            }
            literal => {
                atoms.push(ConditionAtom::Literal(literal));
                index += 1;
            }
        }
    }

    Ok(Condition { atoms })
}

fn empty_marker(value: &str) -> Box<str> {
    Box::<str>::from(if value == "0" { "" } else { value })
}

#[allow(clippy::too_many_lines)]
fn parse_dic(source: &str, text: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<Lexeme> {
    let mut entries = BTreeMap::<Box<str>, BTreeSet<Flag>>::new();
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
        let field = line.split_whitespace().next().unwrap_or_default();
        let (stem, flags) = field
            .split_once('/')
            .map_or((field, None), |(stem, flags)| (stem, Some(flags)));
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
            Some(value) if value.chars().count() > MAX_FLAGS_PER_ENTRY => {
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
                if let Some(flags) = decode_flags(value) {
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
        entries
            .entry(Box::<str>::from(stem))
            .or_default()
            .extend(entry_flags);
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
    entries
        .into_iter()
        .map(|(stem, flags)| Lexeme { stem, flags })
        .collect()
}

fn decode_flags(value: &str) -> Option<BTreeSet<Flag>> {
    (!value.is_empty()).then(|| {
        value
            .chars()
            .map(|character| Flag(Box::<str>::from(character.to_string())))
            .collect()
    })
}

fn decode_flag(value: &str) -> Option<Flag> {
    let mut characters = value.chars();
    let character = characters.next()?;
    characters
        .next()
        .is_none()
        .then(|| Flag(Box::<str>::from(character.to_string())))
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
            | "REP"
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

    use super::{
        import, import_bytes, import_bytes_with_encodings, ByteEncoding, ByteImportEncodings,
        ImportMode, Severity, MAX_AFF_BYTES, MAX_COMPOUND_SCALARS, MAX_DIC_BYTES,
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
    fn compound_rule_quantifiers_remain_explicit_strict_errors() {
        let error = import(
            "test.aff",
            "COMPOUNDRULE 1\nCOMPOUNDRULE A*B\n",
            "test.dic",
            "2\na/A\nb/B\n",
            ImportMode::Strict,
        )
        .expect_err("quantifier syntax is not silently approximated");

        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.directive() == "COMPOUNDRULE"));
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
