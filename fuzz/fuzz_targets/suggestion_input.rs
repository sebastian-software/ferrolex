#![no_main]

use std::sync::OnceLock;

use ferrolex_core::WordList;
use ferrolex_suggest::{SuggestConfig, Suggester};
use libfuzzer_sys::fuzz_target;

fn fuzz_dictionary() -> &'static WordList {
    static DICTIONARY: OnceLock<WordList> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        WordList::new([
            "compound",
            "dictionary",
            "ferrolex",
            "suggestion",
            "unicode",
        ])
        .expect("the static fuzz dictionary is valid")
    })
}

fuzz_target!(|data: &[u8]| {
    let query = String::from_utf8_lossy(data);
    let config = SuggestConfig {
        max_results: 8,
        max_edit_distance: 2,
        max_word_scalars: 256,
        max_candidates: 16,
        max_edit_cells: 16_384,
    };
    let _ = Suggester::new(fuzz_dictionary(), config).suggest(&query);
});
