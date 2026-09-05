use ferrolex::{
    find_locale, import, Checker, Dictionary, HunspellDictionary, ImportMode, SuggestConfig,
    Suggester, WordList,
};

#[test]
fn umbrella_exposes_the_product_api_to_external_consumers() {
    let catalog_entry = find_locale("de_DE").expect("de_DE should be in the built-in catalog");
    assert_eq!(catalog_entry.locale(), "de_DE");

    let imported = import(
        "test.aff",
        "SET UTF-8\n",
        "test.dic",
        "1\nferrolex\n",
        ImportMode::Strict,
    )
    .expect("fixture should import");
    let dictionary: &HunspellDictionary = imported.dictionary();
    assert!(dictionary.contains("ferrolex"));

    let suggester: Suggester<'_, HunspellDictionary> =
        dictionary.suggester(SuggestConfig::default());
    assert_eq!(
        suggester.suggest("ferolex").suggestions()[0].word(),
        "ferrolex"
    );

    let module_dictionary: &ferrolex::hunspell::HunspellDictionary = dictionary;
    assert_eq!(module_dictionary.stems().next(), Some("ferrolex"));
    let _ = ferrolex::suggest::SuggestConfig::default();
    let _ = ferrolex::dictionaries::find_locale("en_US");
}

#[test]
fn checker_composes_layered_suggestion_sources() {
    let checker = Checker::builder()
        .dictionary(WordList::new(["ferrolex"]).expect("fixture should be valid"))
        .dictionary(WordList::new(["ferrous"]).expect("fixture should be valid"))
        .build();

    assert!(checker.contains("ferrolex"));
    assert_eq!(
        Suggester::new(&checker, SuggestConfig::default())
            .suggest("ferolex")
            .suggestions()[0]
            .word(),
        "ferrolex"
    );
}
