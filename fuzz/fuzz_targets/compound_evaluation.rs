#![no_main]

use std::sync::OnceLock;

use ferrolex_core::Dictionary;
use ferrolex_hunspell::{import, HunspellDictionary, ImportMode};
use libfuzzer_sys::fuzz_target;

fn compound_dictionary() -> &'static HunspellDictionary {
    static DICTIONARY: OnceLock<HunspellDictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        import(
            "compound.aff",
            "SET UTF-8\nCOMPOUNDFLAG C\nCOMPOUNDMIN 1\n",
            "compound.dic",
            "2\nfoo/C\nbar/C\n",
            ImportMode::Strict,
        )
        .expect("the static compound fixture imports")
        .dictionary()
        .clone()
    })
}

fuzz_target!(|data: &[u8]| {
    let query = String::from_utf8_lossy(data);
    let _ = compound_dictionary().contains(&query);
});
