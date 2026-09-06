//! Hunspell AFF/DIC parsing and byte decoding.

use super::{
    AffixKind, AffixRule, BTreeSet, ByteEncoding, CaseLanguage, CompoundConfig, CompoundPattern,
    CompoundRule, CompoundSyllableLimit, Condition, ConditionAtom, Cow, Diagnostic, Flag, FlagMode,
    FlagSet, InputConversion, Lexeme, MAX_AFF_BYTES, MAX_AFFIX_ALIASES, MAX_AFFIX_RULES,
    MAX_BREAK_PATTERNS, MAX_CHARACTER_MAPS, MAX_COMPOUND_PATTERNS, MAX_COMPOUND_RULE_COMPONENTS,
    MAX_COMPOUND_RULE_EXPANSIONS, MAX_COMPOUND_RULE_EXPANSIONS_PER_RULE, MAX_COMPOUND_RULES,
    MAX_COMPOUND_SCALARS, MAX_CONDITION_ATOMS, MAX_DIC_BYTES, MAX_DICTIONARY_ENTRIES,
    MAX_FLAGS_PER_ENTRY, MAX_INPUT_CONVERSIONS, MAX_LINE_BYTES, MAX_MORPHOLOGY_FIELDS_PER_RECORD,
    MAX_REPLACEMENT_RULES, Morphology, MorphologyId, MorphologyTable, ReplacementRule, Severity,
    SpecialFlags, encode_text_flag,
};

#[derive(Default)]
pub(crate) struct ParsedAff {
    pub(crate) flag_mode: FlagMode,
    pub(crate) has_flag_mode: bool,
    pub(crate) case_language: CaseLanguage,
    pub(crate) has_language: bool,
    pub(crate) prefixes: Vec<AffixRule>,
    pub(crate) suffixes: Vec<AffixRule>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) rule_count: usize,
    pub(crate) special_flags: SpecialFlags,
    pub(crate) compound: CompoundConfig,
    pub(crate) break_patterns: Vec<BreakPattern>,
    pub(crate) word_characters: BTreeSet<char>,
    pub(crate) replacement_rules: Vec<ReplacementRule>,
    pub(crate) keyboard: Option<Box<str>>,
    pub(crate) character_maps: Vec<String>,
    pub(crate) flag_aliases: Vec<Option<FlagSet>>,
    pub(crate) morphology_aliases: Vec<Option<Morphology>>,
    pub(crate) morphology: MorphologyTable,
    pub(crate) ignored_characters: BTreeSet<char>,
    pub(crate) input_conversions: Vec<InputConversion>,
    pub(crate) output_conversions: Vec<InputConversion>,
    pub(crate) affix_behavior: AffixBehavior,
    pub(crate) declared_sections: BTreeSet<CountedSection>,
}

#[derive(Default)]
pub(crate) struct AffixBehavior {
    pub(crate) full_strip: bool,
    pub(crate) complex_prefixes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BreakPattern {
    pub(crate) text: Box<str>,
    pub(crate) at_start: bool,
    pub(crate) at_end: bool,
}

pub(crate) fn default_break_patterns() -> Vec<BreakPattern> {
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
pub(crate) enum CountedSection {
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
pub(crate) fn parse_aff(source: &str, text: &str) -> ParsedAff {
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

pub(crate) fn parse_flag_aliases(
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

pub(crate) fn parse_morphology_aliases(
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

pub(crate) fn parse_alias_count(fields: &[&str]) -> Option<usize> {
    (fields.len() == 2)
        .then(|| fields[1].parse().ok())
        .flatten()
}

pub(crate) fn parse_input_conversions(
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

pub(crate) fn parse_output_conversions(
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

pub(crate) fn split_conversion_anchors(value: &str) -> (&str, bool, bool) {
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

pub(crate) fn apply_conversions(word: &str, conversions: &[InputConversion]) -> String {
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

pub(crate) fn parse_ignored_characters(
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

pub(crate) fn next_counted_section_line<'source>(
    lines: &mut std::iter::Enumerate<std::str::Lines<'source>>,
) -> Option<(usize, &'source str)> {
    lines.find_map(|(index, line)| (!is_ignored_line(line.trim())).then_some((index, line.trim())))
}

pub(crate) fn normalize_affix_text_for_ignored_characters(
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

pub(crate) fn remove_ignored_characters(
    value: Cow<'_, str>,
    ignored_characters: &BTreeSet<char>,
) -> Box<str> {
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

pub(crate) fn parse_replacement_rules(
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

pub(crate) fn parse_replacement_rule(from: &str, to: &str) -> Option<ReplacementRule> {
    let at_word_start = from.starts_with('^');
    let from = from.strip_prefix('^').unwrap_or(from);
    let at_word_end = from.ends_with('$');
    let from = from.strip_suffix('$').unwrap_or(from);
    ReplacementRule::with_boundaries(from, to, at_word_start, at_word_end)
}

pub(crate) fn parse_keyboard(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
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

pub(crate) fn parse_character_maps(
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

pub(crate) fn parse_unknown_directive(
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

pub(crate) fn parse_set(
    source: &str,
    line: usize,
    fields: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
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

pub(crate) fn parse_flag_mode(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
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

pub(crate) fn parse_language(source: &str, line: usize, fields: &[&str], parsed: &mut ParsedAff) {
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

pub(crate) fn parse_special_flag(
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

pub(crate) fn parse_marker(
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

pub(crate) fn parse_compound_minimum(
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

pub(crate) fn parse_compound_word_maximum(
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

pub(crate) fn parse_compound_syllable_limit(
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

pub(crate) fn parse_compound_patterns(
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

pub(crate) fn parse_compound_pattern(
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

pub(crate) fn parse_compound_pattern_part(
    value: &str,
    flag_mode: FlagMode,
) -> Option<(Box<str>, Option<Flag>)> {
    let (text, flag) = match value.split_once('/') {
        Some((text, flag)) => (text, Some(decode_flag(flag, flag_mode)?)),
        None => (value, None),
    };
    (!text.contains('/')).then(|| (text.into(), flag))
}

pub(crate) fn parse_compound_rules(
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

pub(crate) fn parse_compound_rule_patterns(
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

pub(crate) fn parse_parenthesized_compound_rule(
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

pub(crate) fn parse_break_patterns(
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

pub(crate) fn parse_break_pattern(value: &str) -> Option<BreakPattern> {
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

pub(crate) fn parse_word_characters(
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
pub(crate) fn parse_affix_group(
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
pub(crate) fn parse_affix_rule(
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
            return Err("affix continuation flags exceed the 4096-flag importer limit".to_owned());
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

pub(crate) fn parse_condition(field: &str) -> Result<Condition, String> {
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

pub(crate) fn parse_start_anchor(field: &str) -> Result<(bool, &str), String> {
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

pub(crate) fn parse_negative_lookbehind(
    field: &str,
) -> Result<(Option<ConditionAtom>, &str), String> {
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

pub(crate) fn parse_condition_atoms(field: &str) -> Result<Vec<ConditionAtom>, String> {
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
                return Err("condition uses syntax outside the supported subset".to_owned());
            }
            literal => {
                atoms.push(ConditionAtom::Literal(literal));
                index += 1;
            }
        }
    }
    Ok(atoms)
}

pub(crate) fn parse_condition_atom(field: &str) -> Result<ConditionAtom, String> {
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

pub(crate) fn empty_marker(value: &str) -> Box<str> {
    Box::<str>::from(if value == "0" { "" } else { value })
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "dictionary entry fields and their source-aware diagnostics are parsed together"
)]
pub(crate) fn parse_dic(
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

pub(crate) fn strip_initial_bom(line_index: usize, line: &str) -> &str {
    if line_index == 0 {
        line.strip_prefix('\u{feff}').unwrap_or(line)
    } else {
        line
    }
}

pub(crate) fn decode_entry_flags(
    value: &str,
    flag_mode: FlagMode,
    aliases: &[Option<FlagSet>],
) -> Option<FlagSet> {
    decode_entry_flags_with_count(value, flag_mode, aliases).map(|(flags, _)| flags)
}

pub(crate) fn decode_entry_flags_with_count(
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

pub(crate) fn is_flag_alias_reference(value: &str, aliases: &[Option<FlagSet>]) -> bool {
    !aliases.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn decode_entry_morphology<'field>(
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

pub(crate) fn intern_morphology_fields(
    fields: &[&str],
    table: &mut MorphologyTable,
) -> Result<Vec<MorphologyId>, &'static str> {
    intern_morphology_field_iter(fields.iter().copied(), table)
}

pub(crate) fn intern_morphology_field_iter<'field>(
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

pub(crate) fn decode_flags(value: &str, flag_mode: FlagMode) -> Option<FlagSet> {
    decode_flag_sequence(value, flag_mode).map(flag_set)
}

pub(crate) fn decode_flag_sequence(value: &str, flag_mode: FlagMode) -> Option<Vec<Flag>> {
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

pub(crate) fn flag_set(flags: impl IntoIterator<Item = Flag>) -> FlagSet {
    let mut flags = flags.into_iter().collect::<Vec<_>>();
    flags.sort_unstable();
    flags.dedup();
    flags.into_boxed_slice()
}

pub(crate) fn unicode_flag_tokens(value: &str) -> Option<Vec<&str>> {
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

pub(crate) const fn is_variation_selector(character: char) -> bool {
    matches!(character, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

pub(crate) fn decode_flag(value: &str, flag_mode: FlagMode) -> Option<Flag> {
    let mut flags = decode_flag_sequence(value, flag_mode)?;
    (flags.len() == 1).then(|| flags.pop().expect("one flag"))
}

impl FlagMode {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "UTF-8" | "UTF8" => Some(Self::Unicode),
            "LONG" => Some(Self::Long),
            "NUM" => Some(Self::Numeric),
            _ => None,
        }
    }

    pub(crate) fn flag_count(self, value: &str) -> Option<usize> {
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

pub(crate) fn aff_fields(line: &str) -> Vec<&str> {
    line.split_whitespace()
        .take_while(|field| !field.starts_with('#'))
        .collect()
}

pub(crate) fn is_ignored_line(line: &str) -> bool {
    line.is_empty() || line.starts_with('#')
}

pub(crate) fn is_ignored_dictionary_line(line: &str) -> bool {
    is_ignored_line(line) || line.starts_with('/')
}

pub(crate) fn split_dictionary_entry(field: &str) -> (Cow<'_, str>, Option<&str>) {
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

pub(crate) fn unescape_dictionary_stem(value: &str) -> Cow<'_, str> {
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

pub(crate) fn enforce_input_limit(
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

pub(crate) fn enforce_byte_input_limits(
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

pub(crate) fn enforce_byte_input_limit(
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

pub(crate) fn is_suggestion_only_directive(directive: &str) -> bool {
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

pub(crate) fn diagnostic(
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
