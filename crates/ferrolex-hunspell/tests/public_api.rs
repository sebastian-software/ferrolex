use ferrolex_core::Dictionary;
use ferrolex_hunspell::{
    import, CandidateSource, DictionaryIr, ImportMode, RankingSignals, ReplacementRule,
    SuggestConfig,
};

#[test]
fn reexports_public_api_return_types_for_consumers() {
    let imported = import(
        "test.aff",
        "SET UTF-8\nREP 1\nREP teh the\n",
        "test.dic",
        "1\nferrolex\n",
        ImportMode::Strict,
    )
    .expect("fixture should import");

    let _: &DictionaryIr = imported.ir();
    let _: &[ReplacementRule] = imported.dictionary().replacement_rules();
    let _: RankingSignals<'_> = imported.dictionary().ranking_signals();

    let source: &dyn CandidateSource = imported.dictionary();
    assert!(source.contains_candidate("ferrolex"));
}

#[test]
fn import_result_can_transfer_dictionary_ownership() {
    let imported = import(
        "test.aff",
        "SET UTF-8\n",
        "test.dic",
        "1\nferrolex\n",
        ImportMode::Strict,
    )
    .expect("fixture should import");

    let dictionary = imported.into_dictionary();
    assert!(dictionary.contains("ferrolex"));
}

#[test]
fn dictionary_suggester_preserves_hunspell_suggestion_metadata() {
    let imported = import(
        "test.aff",
        "SET UTF-8\nREP 1\nREP teh the\nOCONV 1\nOCONV the æ\n",
        "test.dic",
        "1\nthe\n",
        ImportMode::Strict,
    )
    .expect("fixture should import");
    let dictionary = imported.dictionary();

    let result = dictionary
        .suggester(SuggestConfig {
            max_edit_distance: 0,
            ..SuggestConfig::default()
        })
        .suggest("teh");

    assert_eq!(result.suggestions()[0].word(), "the");
    assert_eq!(
        dictionary.normalize_output(result.suggestions()[0].word()),
        "æ"
    );
}
