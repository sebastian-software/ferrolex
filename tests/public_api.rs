use ferrolex::{
    catalog_import_encodings, find_locale, import, ByteEncoding, Checker, Dictionary,
    HunspellDictionary, ImportMode, SourceEncoding, SuggestConfig, Suggester, WordList,
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

#[test]
fn catalog_encoding_mapping_is_available_to_library_consumers() {
    let mixed_latin = catalog_import_encodings(SourceEncoding::MixedUtf8AndIso8859_1)
        .expect("mixed catalog encoding needs an explicit policy");
    assert_eq!(mixed_latin.aff(), ByteEncoding::Iso8859_1);
    assert_eq!(mixed_latin.dic(), ByteEncoding::Utf8);

    let mixed_central_european =
        catalog_import_encodings(SourceEncoding::MixedUtf8AndIso8859_2Fallback)
            .expect("fallback catalog encoding needs an explicit policy");
    assert_eq!(
        mixed_central_european.aff(),
        ByteEncoding::Utf8WithIso8859_2Fallback
    );
    assert_eq!(mixed_central_european.dic(), ByteEncoding::Utf8);
    assert!(catalog_import_encodings(SourceEncoding::Utf8).is_none());
}
