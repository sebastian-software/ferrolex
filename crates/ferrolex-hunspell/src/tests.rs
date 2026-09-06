use std::borrow::Cow;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread;

use ferrolex_core::Dictionary;
use ferrolex_suggest::{CandidateSource, Completeness, SuggestConfig, Suggester};

use super::{
    compile_runtime_cache, import, import_bytes, import_bytes_with_encodings, load_runtime_cache,
    AcceptanceKind, AppliedAffixKind, ByteEncoding, ByteImportEncodings, CasingPath, ImportMode,
    LookupExplanation, RejectionReason, Severity, SourceDigests, MAX_AFF_BYTES,
    MAX_COMPOUND_SCALARS, MAX_DERIVED_CANDIDATES_PER_LOOKUP, MAX_DIC_BYTES, MAX_FLAGS_PER_ENTRY,
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
    let decoded = flags.map(|flag| super::decode_text_flag(flag).expect("encoded flag decodes"));

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
    assert!(result.diagnostics().iter().any(
        |diagnostic| diagnostic.directive() == "AF" && diagnostic.severity() == Severity::Error
    ));
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
    let cache =
        compile_runtime_cache(imported.dictionary(), source_digests).expect("the cache compiles");
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
    let loaded =
        load_runtime_cache(&cache, sources).expect("complex prefixes load from the runtime cache");
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
        diagnostic.directive() == "COMPOUNDRULE" && diagnostic.message().contains("per-rule limit")
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
