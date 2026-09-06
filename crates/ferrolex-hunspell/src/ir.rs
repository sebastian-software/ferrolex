//! Lowering helpers for the source-neutral dictionary IR.

use super::{
    decode_text_flag, AffixKind, AffixKindIr, AffixRule, AffixRuleIr, BTreeSet, BreakPattern,
    BreakPatternIr, CaseLanguage, CaseLanguageIr, CompoundConfig, CompoundConfigIr,
    CompoundPattern, CompoundPatternIr, CompoundSyllableLimitIr, Condition, ConditionAtom,
    ConditionAtomIr, ConditionIr, Flag, FlagIr, FlagMode, InputConversion, Lexeme, LexemeIr,
    Morphology, ReplacementRule, ReplacementRuleIr, SpecialFlags, SpecialFlagsIr,
};
use ferrolex_compiler::{FlagModeIr, InputConversionIr};

pub(crate) fn flag_mode_to_ir(mode: FlagMode) -> FlagModeIr {
    match mode {
        FlagMode::Unicode => FlagModeIr::Unicode,
        FlagMode::Long => FlagModeIr::Long,
        FlagMode::Numeric => FlagModeIr::Numeric,
    }
}

pub(crate) fn case_language_to_ir(language: CaseLanguage) -> CaseLanguageIr {
    match language {
        CaseLanguage::Default => CaseLanguageIr::Default,
        CaseLanguage::Turkic => CaseLanguageIr::Turkic,
    }
}

pub(crate) fn flag_to_ir(flag: Flag, mode: FlagMode) -> FlagIr {
    match mode {
        FlagMode::Numeric => {
            FlagIr::Numeric(u32::try_from(flag.0).expect("validated numeric flags fit in a u32"))
        }
        FlagMode::Unicode | FlagMode::Long => FlagIr::Text(
            decode_text_flag(flag.0).expect("validated text flags contain Unicode scalars"),
        ),
    }
}

pub(crate) fn flags_to_ir(flags: &[Flag], mode: FlagMode) -> BTreeSet<FlagIr> {
    flags
        .iter()
        .copied()
        .map(|flag| flag_to_ir(flag, mode))
        .collect()
}

pub(crate) fn morphology_to_ir(morphology: &Morphology) -> Vec<u32> {
    morphology.iter().map(|id| id.0).collect()
}

pub(crate) fn lexeme_to_ir(lexeme: &Lexeme, flag_mode: FlagMode) -> LexemeIr {
    LexemeIr {
        stem: lexeme.stem.to_string(),
        frequency: None,
        flags: flags_to_ir(&lexeme.flags, flag_mode),
        morphology: morphology_to_ir(&lexeme.morphology),
    }
}

pub(crate) fn affix_rule_to_ir(rule: &AffixRule, flag_mode: FlagMode) -> AffixRuleIr {
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

pub(crate) fn condition_to_ir(condition: &Condition) -> ConditionIr {
    ConditionIr {
        atoms: condition.atoms.iter().map(condition_atom_to_ir).collect(),
        not_preceded_by: condition.not_preceded_by.as_ref().map(condition_atom_to_ir),
        anchored_at_start: condition.anchored_at_start,
    }
}

pub(crate) fn condition_atom_to_ir(atom: &ConditionAtom) -> ConditionAtomIr {
    match atom {
        ConditionAtom::Any => ConditionAtomIr::Any,
        ConditionAtom::Literal(character) => ConditionAtomIr::Literal(*character),
        ConditionAtom::Class { members, negated } => ConditionAtomIr::Class {
            members: members.clone(),
            negated: *negated,
        },
    }
}

pub(crate) fn special_flags_to_ir(flags: &SpecialFlags, flag_mode: FlagMode) -> SpecialFlagsIr {
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

pub(crate) fn compound_to_ir(compound: &CompoundConfig, flag_mode: FlagMode) -> CompoundConfigIr {
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

pub(crate) fn compound_pattern_to_ir(
    pattern: &CompoundPattern,
    flag_mode: FlagMode,
) -> CompoundPatternIr {
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

pub(crate) fn break_pattern_to_ir(pattern: &BreakPattern) -> BreakPatternIr {
    BreakPatternIr {
        text: pattern.text.to_string(),
        at_start: pattern.at_start,
        at_end: pattern.at_end,
    }
}

pub(crate) fn input_conversion_to_ir(conversion: &InputConversion) -> InputConversionIr {
    InputConversionIr {
        from: conversion.from.to_string(),
        to: conversion.to.to_string(),
        at_word_start: conversion.at_word_start,
        at_word_end: conversion.at_word_end,
    }
}

pub(crate) fn replacement_rule_to_ir(rule: &ReplacementRule) -> ReplacementRuleIr {
    ReplacementRuleIr {
        from: rule.from().to_owned(),
        to: rule.to().to_owned(),
        at_word_start: rule.at_word_start(),
        at_word_end: rule.at_word_end(),
    }
}
