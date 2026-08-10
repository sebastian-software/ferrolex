//! Hunspell-compatible dictionary import for ferrolex.
//!
//! The importer accepts a deliberately documented subset of the textual
//! Hunspell format and translates it into ferrolex-owned data structures. No
//! runtime dependency on another spell checker is introduced.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ferrolex_core::Dictionary;

const MAX_AFF_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIC_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_AFFIX_RULES: usize = 100_000;
const MAX_DICTIONARY_ENTRIES: usize = 1_000_000;
const MAX_FLAGS_PER_ENTRY: usize = 256;
const MAX_CONDITION_ATOMS: usize = 256;
const MAX_AFFIX_CHAIN: usize = 8;
const MAX_COMPOUND_SCALARS: usize = 256;

/// Selects whether importer diagnostics prevent a dictionary from loading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImportMode {
    /// Return supported content and all diagnostics.
    #[default]
    Lenient,
    /// Reject an import that has an error diagnostic.
    Strict,
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
    lexemes: Vec<Lexeme>,
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
}

impl Dictionary for HunspellDictionary {
    fn contains(&self, word: &str) -> bool {
        self.lexemes
            .iter()
            .any(|lexeme| self.matches_word_from_lexeme(lexeme, word))
            || self.matches_simple_compound(word)
    }
}

impl HunspellDictionary {
    fn matches_word_from_lexeme(&self, lexeme: &Lexeme, word: &str) -> bool {
        if self.is_forbidden(&lexeme.flags) {
            return false;
        }
        let mut states = vec![FormState::new(lexeme)];

        while let Some(state) = states.pop() {
            if state.form == word && self.is_accepted_state(&state) {
                return true;
            }
            if state.depth == MAX_AFFIX_CHAIN {
                continue;
            }
            for rule in self.prefixes.iter().chain(&self.suffixes) {
                if !state.flags.contains(&rule.flag) || !state.can_apply(rule) {
                    continue;
                }
                if let Some(form) = rule.apply(&state.form) {
                    states.push(state.apply(rule, form, &self.special_flags));
                }
            }
        }
        false
    }

    fn is_accepted_state(&self, state: &FormState) -> bool {
        !self.is_forbidden(&state.flags)
            && (!self.requires_affix(&state.origin_flags) || state.depth > 0)
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

    fn matches_simple_compound(&self, word: &str) -> bool {
        let Some(compound_flag) = &self.compound.flag else {
            return false;
        };
        let characters = word.char_indices().collect::<Vec<_>>();
        if characters.len() > MAX_COMPOUND_SCALARS {
            return false;
        }
        let minimum = self.compound.minimum_length;
        (1..characters.len()).any(|split| {
            let byte_index = characters[split].0;
            let (left, right) = word.split_at(byte_index);
            left.chars().count() >= minimum
                && right.chars().count() >= minimum
                && self.lexemes.iter().any(|lexeme| {
                    lexeme.stem.as_ref() == left
                        && lexeme.flags.contains(compound_flag)
                        && !self.is_forbidden(&lexeme.flags)
                })
                && self.lexemes.iter().any(|lexeme| {
                    lexeme.stem.as_ref() == right
                        && lexeme.flags.contains(compound_flag)
                        && !self.is_forbidden(&lexeme.flags)
                })
        })
    }
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
            && (self.last_kind.is_none()
                || self.last_kind == Some(rule.kind)
                || (self.last_cross_product && rule.cross_product))
    }

    fn apply(&self, rule: &AffixRule, form: String, special_flags: &SpecialFlags) -> Self {
        let circumfix = special_flags
            .circumfix
            .as_ref()
            .is_some_and(|flag| rule.continuation_flags.contains(flag));
        let mut flags = self.flags.clone();
        flags.extend(rule.continuation_flags.iter().cloned());
        let mut used_rules = self.used_rules.clone();
        used_rules.insert(rule.id);
        Self {
            form,
            flags,
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
}

#[derive(Clone, Debug)]
struct CompoundConfig {
    flag: Option<Flag>,
    minimum_length: usize,
}

impl Default for CompoundConfig {
    fn default() -> Self {
        Self {
            flag: None,
            minimum_length: 3,
        }
    }
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
    let mut diagnostics = Vec::new();
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
    let dictionary = HunspellDictionary {
        lexemes,
        prefixes: parsed_aff.prefixes,
        suffixes: parsed_aff.suffixes,
        special_flags: parsed_aff.special_flags,
        compound: parsed_aff.compound,
    };

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

#[derive(Default)]
struct ParsedAff {
    prefixes: Vec<AffixRule>,
    suffixes: Vec<AffixRule>,
    diagnostics: Vec<Diagnostic>,
    rule_count: usize,
    special_flags: SpecialFlags,
    compound: CompoundConfig,
}

fn parse_aff(source: &str, text: &str) -> ParsedAff {
    let lines = text.lines().collect::<Vec<_>>();
    let mut parsed = ParsedAff::default();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].trim();
        let line_number = index + 1;
        index += 1;
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
            "COMPOUNDFLAG" => parse_special_flag(
                source,
                line_number,
                directive,
                &fields,
                &mut parsed.compound.flag,
                &mut parsed.diagnostics,
            ),
            "COMPOUNDMIN" => parse_compound_minimum(
                source,
                line_number,
                &fields,
                &mut parsed.compound,
                &mut parsed.diagnostics,
            ),
            "PFX" | "SFX" => parse_affix_group(
                source,
                directive,
                &lines,
                &mut index,
                line_number,
                &fields,
                &mut parsed,
            ),
            _ => parsed.diagnostics.push(diagnostic(
                source,
                line_number,
                directive,
                if is_suggestion_only_directive(directive) {
                    Severity::Warning
                } else {
                    Severity::Error
                },
                if is_suggestion_only_directive(directive) {
                    "suggestion-only directive is not implemented in the current compatibility level"
                } else {
                    "directive may affect recognition and is not implemented in the current compatibility level"
                },
            )),
        }
    }

    parsed
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
    } else if !matches!(fields[1].to_ascii_uppercase().as_str(), "UTF-8" | "UTF8") {
        diagnostics.push(diagnostic(
            source,
            line,
            "SET",
            Severity::Error,
            "only UTF-8 input is supported by this importer",
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_affix_group(
    source: &str,
    directive: &str,
    lines: &[&str],
    index: &mut usize,
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
    while *index < lines.len() && consumed_rules < rule_count {
        let rule_line = lines[*index].trim();
        let rule_line_number = *index + 1;
        *index += 1;
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
    use std::sync::Arc;
    use std::thread;

    use ferrolex_core::Dictionary;

    use super::{import, ImportMode, Severity};

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
        let affixes = "SET ISO-8859-1\nCOMPOUNDMIN 3\n";
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
