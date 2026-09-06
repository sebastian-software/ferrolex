//! Internal Hunspell runtime model and casing primitives.

use super::{
    BTreeMap, BTreeSet, Cow, MAX_AFFIX_CHAIN, MAX_MORPHOLOGY_STRINGS, is_variation_selector,
};

#[derive(Clone, Debug)]
pub(crate) struct Lexeme {
    pub(crate) stem: Box<str>,
    pub(crate) flags: FlagSet,
    pub(crate) morphology: Morphology,
}

pub(crate) type Morphology = Box<[MorphologyId]>;
pub(crate) type FlagSet = Box<[Flag]>;

pub(crate) fn has_flag(flags: &[Flag], flag: Flag) -> bool {
    flags.binary_search(&flag).is_ok()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MorphologyId(pub(crate) u32);

/// Stores each morphology field only once while keeping stable compact IDs in
/// dictionary entries and affix rules.
#[derive(Clone, Debug, Default)]
pub(crate) struct MorphologyTable {
    pub(crate) ids: BTreeMap<Box<str>, MorphologyId>,
}

impl MorphologyTable {
    pub(crate) fn intern(&mut self, field: &str) -> Option<MorphologyId> {
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

    pub(crate) fn contains(&self, id: MorphologyId) -> bool {
        usize::try_from(id.0).is_ok_and(|index| index < self.ids.len())
    }

    pub(crate) fn values_by_id(&self) -> Vec<&str> {
        let mut values = vec![""; self.ids.len()];
        for (value, id) in &self.ids {
            values[usize::try_from(id.0).expect("morphology ID fits usize")] = value;
        }
        values
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Flag(pub(crate) u64);

pub(crate) fn encode_text_flag(value: &str) -> Option<u64> {
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

pub(crate) fn decode_text_flag(value: u64) -> Option<String> {
    let (first, second) = decode_text_flag_chars(value)?;
    let mut decoded = String::with_capacity(8);
    decoded.push(first);
    if let Some(second) = second {
        decoded.push(second);
    }
    Some(decoded)
}

pub(crate) fn decode_text_flag_chars(value: u64) -> Option<(char, Option<char>)> {
    let first = char::from_u32(u32::try_from(value >> 32).ok()?)?;
    let encoded_second = u32::try_from(value & u64::from(u32::MAX)).ok()?;
    let second = (encoded_second != 0)
        .then(|| char::from_u32(encoded_second - 1))
        .flatten();
    (encoded_second == 0 || second.is_some()).then_some((first, second))
}

impl Flag {
    pub(crate) fn is_valid_for(self, mode: FlagMode) -> bool {
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
pub(crate) enum FlagMode {
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
pub(crate) enum CaseLanguage {
    #[default]
    Default,
    Turkic,
}

impl CaseLanguage {
    pub(crate) fn from_lang(value: &str) -> Self {
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
pub(crate) enum CasePattern {
    Initial,
    Upper,
}

pub(crate) fn case_pattern(word: &str, language: CaseLanguage) -> Option<CasePattern> {
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

pub(crate) fn is_cased(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        match character {
            'I' | 'İ' | 'i' | 'ı' => return true,
            _ => {}
        }
    }
    !character.to_lowercase().eq(character.to_uppercase())
}

pub(crate) fn is_uppercase(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        return matches!(character, 'I' | 'İ')
            || (!matches!(character, 'i' | 'ı') && lowercase_changes(character));
    }
    lowercase_changes(character)
}

pub(crate) fn is_lowercase(character: char, language: CaseLanguage) -> bool {
    if language == CaseLanguage::Turkic {
        return matches!(character, 'i' | 'ı')
            || (!matches!(character, 'I' | 'İ') && uppercase_changes(character));
    }
    uppercase_changes(character)
}

pub(crate) fn lowercase_changes(character: char) -> bool {
    let mut lowercase = character.to_lowercase();
    lowercase.next() != Some(character) || lowercase.next().is_some()
}

pub(crate) fn uppercase_changes(character: char) -> bool {
    let mut uppercase = character.to_uppercase();
    uppercase.next() != Some(character) || uppercase.next().is_some()
}

pub(crate) fn lowercase_for_language(word: &str, language: CaseLanguage) -> String {
    let mut result = String::with_capacity(word.len());
    for character in word.chars() {
        push_lowercase(&mut result, character, language);
    }
    result
}

pub(crate) fn initial_case_for_language(word: &str, language: CaseLanguage) -> String {
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

pub(crate) fn push_lowercase(result: &mut String, character: char, language: CaseLanguage) {
    if language == CaseLanguage::Turkic {
        match character {
            'I' => return result.push('ı'),
            'İ' => return result.push('i'),
            _ => {}
        }
    }
    result.extend(character.to_lowercase());
}

pub(crate) fn push_uppercase(result: &mut String, character: char, language: CaseLanguage) {
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
pub(crate) struct InputConversion {
    pub(crate) from: Box<str>,
    pub(crate) to: Box<str>,
    pub(crate) at_word_start: bool,
    pub(crate) at_word_end: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AffixKind {
    Prefix,
    Suffix,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum CompoundPosition {
    Begin,
    Middle,
    End,
}

#[derive(Clone, Debug)]
pub(crate) struct AffixRule {
    pub(crate) id: usize,
    pub(crate) kind: AffixKind,
    pub(crate) flag: Flag,
    pub(crate) strip: Box<str>,
    pub(crate) add: Box<str>,
    pub(crate) condition: Condition,
    pub(crate) cross_product: bool,
    pub(crate) continuation_flags: FlagSet,
    pub(crate) morphology: Morphology,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AffixRuleIndex {
    pub(crate) empty_add: Vec<usize>,
    pub(crate) by_add_edge: BTreeMap<char, Vec<usize>>,
}

impl AffixRuleIndex {
    pub(crate) fn new(rules: &[AffixRule], kind: AffixKind) -> Self {
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

    pub(crate) fn matching_rules<'source>(
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
    pub(crate) fn could_generate(&self, word: &str) -> bool {
        match self.kind {
            AffixKind::Prefix => word.starts_with(self.add.as_ref()),
            AffixKind::Suffix => word.ends_with(self.add.as_ref()),
        }
    }

    pub(crate) fn apply(&self, stem: &str, full_strip: bool) -> Option<String> {
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

    pub(crate) fn reverse_apply<'form>(
        &self,
        form: &'form str,
        full_strip: bool,
    ) -> Option<Cow<'form, str>> {
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
pub(crate) struct FormState<'source> {
    pub(crate) form: String,
    pub(crate) flags: &'source [Flag],
    pub(crate) origin_flags: &'source [Flag],
    pub(crate) depth: usize,
    pub(crate) prefix_count: usize,
    pub(crate) suffix_count: usize,
    pub(crate) last_kind: Option<AffixKind>,
    pub(crate) last_cross_product: bool,
    pub(crate) used_rules: [usize; MAX_AFFIX_CHAIN],
    pub(crate) circumfix_prefix: bool,
    pub(crate) circumfix_suffix: bool,
}

impl<'source> FormState<'source> {
    pub(crate) fn new(lexeme: &'source Lexeme) -> Self {
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

    pub(crate) fn can_apply(&self, rule: &AffixRule, complex_prefixes: bool) -> bool {
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

    pub(crate) fn flags_for(&self, kind: AffixKind) -> &[Flag] {
        match self.last_kind {
            Some(previous_kind) if previous_kind != kind => self.origin_flags,
            Some(_) | None => self.flags,
        }
    }

    pub(crate) fn apply(
        &self,
        rule: &'source AffixRule,
        form: String,
        special_flags: &SpecialFlags,
    ) -> Self {
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

    pub(crate) fn has_complete_circumfix(&self) -> bool {
        self.circumfix_prefix == self.circumfix_suffix
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SpecialFlags {
    pub(crate) circumfix: Option<Flag>,
    pub(crate) forbidden_word: Option<Flag>,
    pub(crate) keep_case: Option<Flag>,
    pub(crate) need_affix: Option<Flag>,
    pub(crate) only_in_compound: Option<Flag>,
    pub(crate) no_suggest: Option<Flag>,
    pub(crate) check_sharps: bool,
}

#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each imported Hunspell marker is an independent recognition safeguard"
)]
pub(crate) struct CompoundConfig {
    pub(crate) flag: Option<Flag>,
    pub(crate) begin: Option<Flag>,
    pub(crate) middle: Option<Flag>,
    pub(crate) end: Option<Flag>,
    pub(crate) permit: Option<Flag>,
    pub(crate) forbid: Option<Flag>,
    pub(crate) force_uppercase: Option<Flag>,
    pub(crate) minimum_length: usize,
    pub(crate) maximum_words: Option<usize>,
    pub(crate) check_duplicate: bool,
    pub(crate) check_replacement: bool,
    pub(crate) check_case: bool,
    pub(crate) check_triple: bool,
    pub(crate) simplified_triple: bool,
    pub(crate) patterns: Vec<CompoundPattern>,
    pub(crate) syllable_limit: Option<CompoundSyllableLimit>,
    pub(crate) rules: Vec<CompoundRule>,
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
pub(crate) struct CompoundPattern {
    pub(crate) ending: Box<str>,
    pub(crate) ending_flag: Option<Flag>,
    pub(crate) beginning: Box<str>,
    pub(crate) beginning_flag: Option<Flag>,
    pub(crate) replacement: Option<Box<str>>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompoundSyllableLimit {
    pub(crate) maximum: usize,
    pub(crate) vowels: BTreeSet<char>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompoundRule {
    pub(crate) patterns: Vec<Vec<Flag>>,
}

#[derive(Clone, Debug)]
pub(crate) struct Condition {
    pub(crate) atoms: Vec<ConditionAtom>,
    pub(crate) not_preceded_by: Option<ConditionAtom>,
    pub(crate) anchored_at_start: bool,
}

impl Condition {
    pub(crate) fn empty() -> Self {
        Self {
            atoms: Vec::new(),
            not_preceded_by: None,
            anchored_at_start: false,
        }
    }

    pub(crate) fn matches(&self, stem: &str, kind: AffixKind) -> bool {
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
pub(crate) enum ConditionAtom {
    Any,
    Literal(char),
    Class {
        members: BTreeSet<char>,
        negated: bool,
    },
}

impl ConditionAtom {
    pub(crate) fn matches(&self, character: char) -> bool {
        match self {
            Self::Any => true,
            Self::Literal(expected) => *expected == character,
            Self::Class { members, negated } => members.contains(&character) != *negated,
        }
    }
}
