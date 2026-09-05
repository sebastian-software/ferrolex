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
        "SET UTF-8\nKEY qw|er\nMAP 1\nMAP áz\nREP 1\nREP teh the\nOCONV 1\nOCONV the æ\n",
        "test.dic",
        "5\nthe\ne\nw\na\nz\n",
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

    assert_eq!(
        dictionary
            .suggester(SuggestConfig::default())
            .suggest("q")
            .suggestions()[0]
            .word(),
        "w"
    );
    assert_eq!(
        dictionary
            .suggester(SuggestConfig::default())
            .suggest("á")
            .suggestions()[0]
            .word(),
        "z"
    );
}

#[test]
fn import_errors_render_actionable_diagnostics() {
    let error = import(
        "unsupported.aff",
        "SET KOI8-R\n",
        "unsupported.dic",
        "1\nword\n",
        ImportMode::Strict,
    )
    .expect_err("unsupported byte encodings are strict import failures");

    assert_eq!(
        error.diagnostics()[0].to_string(),
        "unsupported.aff:1: error[SET]: SET must name UTF-8, ISO-8859-1, or ISO-8859-2"
    );
    assert_eq!(
        error.to_string(),
        "Hunspell import failed with 1 diagnostic(s); first: unsupported.aff:1: error[SET]: SET must name UTF-8, ISO-8859-1, or ISO-8859-2"
    );
}
