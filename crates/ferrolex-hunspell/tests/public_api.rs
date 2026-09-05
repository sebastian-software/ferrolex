use ferrolex_hunspell::{
    import, CandidateSource, DictionaryIr, ImportMode, RankingSignals, ReplacementRule,
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
