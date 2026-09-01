#![no_main]

use std::sync::OnceLock;

use ferrolex_code::{Analyzer, Document, ProjectConfig};
use ferrolex_core::WordList;
use libfuzzer_sys::fuzz_target;

fn fuzz_dictionary() -> &'static WordList {
    static DICTIONARY: OnceLock<WordList> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        WordList::new(["config", "dictionary", "ferrolex", "project"])
            .expect("the static fuzz dictionary is valid")
    })
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(config) = ProjectConfig::from_text(&text) {
        let canonical = config.to_text();
        let reparsed = ProjectConfig::from_text(&canonical)
            .expect("canonical project configuration must parse");
        assert_eq!(config, reparsed);

        if let Ok(builder) = Analyzer::builder(fuzz_dictionary()).project_config(&config) {
            let _ = builder.build().check(&Document::new(&text));
        }
    }
});
