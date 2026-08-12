//! Source-neutral linguistic dictionary intermediate representation.
//!
//! The IR owns only declared dictionary semantics. Runtime indexes and derived
//! lookup caches are deliberately excluded so importers, compilers, and future
//! artifact formats can make their own storage decisions without losing source
//! meaning.

use std::collections::BTreeSet;

/// A source-neutral linguistic dictionary ready for artifact compilation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DictionaryIr {
    /// Flag encoding used by every flag-bearing field.
    pub flag_mode: FlagModeIr,
    /// Whether capitalization fallback is available.
    pub case_fallback: bool,
    /// Language-specific casing behavior.
    pub case_language: CaseLanguageIr,
    /// Interned morphology strings referenced by lexemes and affix rules.
    pub morphology: Vec<String>,
    /// Stored stems and their declared attributes.
    pub lexemes: Vec<LexemeIr>,
    /// Prefix transformation rules.
    pub prefixes: Vec<AffixRuleIr>,
    /// Suffix transformation rules.
    pub suffixes: Vec<AffixRuleIr>,
    /// Recognition-affecting marker flags.
    pub special_flags: SpecialFlagsIr,
    /// Compound-word configuration.
    pub compound: CompoundConfigIr,
    /// Declared break patterns.
    pub break_patterns: Vec<BreakPatternIr>,
    /// Extra word characters retained as tokenizer metadata.
    pub word_characters: BTreeSet<char>,
    /// Suggestion-ranking replacement rules.
    pub replacement_rules: Vec<ReplacementRuleIr>,
    /// Keyboard layout retained for suggestion ranking only.
    pub keyboard: Option<String>,
    /// Character-equivalence groups retained for suggestion ranking only.
    pub character_maps: Vec<String>,
    /// Characters removed before lookup and from imported spellings.
    pub ignored_characters: BTreeSet<char>,
    /// Input normalizations applied before lookup.
    pub input_conversions: Vec<InputConversionIr>,
    /// Output normalizations used only when rendering suggestions.
    pub output_conversions: Vec<InputConversionIr>,
    /// Whether affix stripping may consume a complete stem.
    pub full_strip: bool,
    /// Whether a derived form may contain two prefixes.
    pub complex_prefixes: bool,
}

/// A stored stem and its declared flags and morphology references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexemeIr {
    /// UTF-8 stem spelling.
    pub stem: String,
    /// Flags attached to the stored stem.
    pub flags: BTreeSet<FlagIr>,
    /// Zero-based references into [`DictionaryIr::morphology`].
    pub morphology: Vec<u32>,
}

/// A Hunspell flag in a source-neutral representation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FlagIr {
    /// Positive numeric flag.
    Numeric(u32),
    /// Textual flag interpreted according to [`FlagModeIr`].
    Text(String),
}

/// Encoding used for dictionary flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FlagModeIr {
    /// One Unicode scalar per flag.
    #[default]
    Unicode,
    /// Two Unicode scalars per flag.
    Long,
    /// Positive comma-separated numeric flags.
    Numeric,
}

/// Language-specific casing behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CaseLanguageIr {
    /// Unicode default casing.
    #[default]
    Default,
    /// Turkish-family dotted and dotless-I casing.
    Turkic,
}

/// A declared affix transformation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AffixRuleIr {
    /// Stable rule ID within the dictionary.
    pub id: u32,
    /// Whether this is a prefix or suffix rule.
    pub kind: AffixKindIr,
    /// Flag that enables the rule.
    pub flag: FlagIr,
    /// Text removed before adding the affix.
    pub strip: String,
    /// Text added by the rule.
    pub add: String,
    /// Condition evaluated against the source stem.
    pub condition: ConditionIr,
    /// Whether this rule may cross-product with the other affix kind.
    pub cross_product: bool,
    /// Flags active on the generated form.
    pub continuation_flags: BTreeSet<FlagIr>,
    /// Zero-based references into [`DictionaryIr::morphology`].
    pub morphology: Vec<u32>,
}

/// The side of a word to which an affix applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AffixKindIr {
    /// Applies before the stem.
    Prefix,
    /// Applies after the stem.
    Suffix,
}

/// A bounded affix condition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionIr {
    /// Required scalar sequence.
    pub atoms: Vec<ConditionAtomIr>,
    /// Optional one-scalar negative lookbehind.
    pub not_preceded_by: Option<ConditionAtomIr>,
    /// Whether the condition is anchored at word start.
    pub anchored_at_start: bool,
}

/// One condition matcher atom.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConditionAtomIr {
    /// Any Unicode scalar.
    Any,
    /// One exact scalar.
    Literal(char),
    /// A character class, optionally negated.
    Class {
        /// Class members.
        members: BTreeSet<char>,
        /// Whether class membership must not match.
        negated: bool,
    },
}

/// Recognition-affecting special flags.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpecialFlagsIr {
    /// Circumfix continuation marker.
    pub circumfix: Option<FlagIr>,
    /// Stored-word rejection marker.
    pub forbidden_word: Option<FlagIr>,
    /// Casing-fallback exclusion marker.
    pub keep_case: Option<FlagIr>,
    /// Requires a derived form marker.
    pub need_affix: Option<FlagIr>,
    /// Valid only in compounds marker.
    pub only_in_compound: Option<FlagIr>,
    /// Suppresses a recognized spelling from suggestion output.
    pub no_suggest: Option<FlagIr>,
    /// Enables sharp-S casing handling.
    pub check_sharps: bool,
}

/// Compound-word configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each imported Hunspell compound marker has independent semantics"
)]
pub struct CompoundConfigIr {
    /// General compound member marker.
    pub flag: Option<FlagIr>,
    /// First-position marker.
    pub begin: Option<FlagIr>,
    /// Middle-position marker.
    pub middle: Option<FlagIr>,
    /// Last-position marker.
    pub end: Option<FlagIr>,
    /// Affix-produced compound permission marker.
    pub permit: Option<FlagIr>,
    /// Compound rejection marker.
    pub forbid: Option<FlagIr>,
    /// Compound uppercase marker.
    pub force_uppercase: Option<FlagIr>,
    /// Minimum scalar length of a compound component.
    pub minimum_length: usize,
    /// Optional maximum component count.
    pub maximum_words: Option<usize>,
    /// Reject repeated components.
    pub check_duplicate: bool,
    /// Apply compound replacement checks.
    pub check_replacement: bool,
    /// Apply compound casing checks.
    pub check_case: bool,
    /// Reject triple-letter boundaries.
    pub check_triple: bool,
    /// Use simplified triple-letter behavior.
    pub simplified_triple: bool,
    /// Declared compound boundary patterns.
    pub patterns: Vec<CompoundPatternIr>,
    /// Optional syllable limit.
    pub syllable_limit: Option<CompoundSyllableLimitIr>,
    /// Allowed bounded component-flag sequences.
    pub rules: Vec<Vec<Vec<FlagIr>>>,
}

impl Default for CompoundConfigIr {
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

/// A compound boundary pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompoundPatternIr {
    /// Ending spelling, optionally flag-qualified.
    pub ending: String,
    /// Optional flag required on the ending component.
    pub ending_flag: Option<FlagIr>,
    /// Beginning spelling, optionally flag-qualified.
    pub beginning: String,
    /// Optional flag required on the beginning component.
    pub beginning_flag: Option<FlagIr>,
    /// Optional replacement spelling.
    pub replacement: Option<String>,
}

/// A maximum compound syllable count and its vowel set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompoundSyllableLimitIr {
    /// Maximum permitted syllables.
    pub maximum: usize,
    /// Unicode scalars counted as vowels.
    pub vowels: BTreeSet<char>,
}

/// A literal break pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreakPatternIr {
    /// Pattern spelling.
    pub text: String,
    /// Whether it only applies at word start.
    pub at_start: bool,
    /// Whether it only applies at word end.
    pub at_end: bool,
}

/// A literal input or output conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputConversionIr {
    /// Source spelling.
    pub from: String,
    /// Replacement spelling.
    pub to: String,
    /// Whether the source must begin the word.
    pub at_word_start: bool,
    /// Whether the source must end the word.
    pub at_word_end: bool,
}

/// A suggestion-ranking spelling replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementRuleIr {
    /// Typo spelling.
    pub from: String,
    /// Preferred spelling.
    pub to: String,
    /// Whether the typo spelling must begin the word.
    pub at_word_start: bool,
    /// Whether the typo spelling must end the word.
    pub at_word_end: bool,
}
