#![no_main]

use ferrolex_code::{split_identifier, IdentifierSplitConfig};
use ferrolex_core::{Dictionary, Normalization, WordList};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    for normalization in [
        Normalization::Exact,
        Normalization::Nfc,
        Normalization::Nfkc,
    ] {
        let dictionary = WordList::from_text(normalization, &text);
        for word in dictionary.words() {
            assert!(dictionary.contains(word));
        }
    }

    for segment in split_identifier(&text, IdentifierSplitConfig::default()) {
        assert_eq!(segment.text(), &text[segment.range()]);
    }
});
