#![no_main]

use std::sync::OnceLock;

use ferrolex_code::{Analyzer, CommentSyntax, Document};
use ferrolex_core::WordList;
use libfuzzer_sys::fuzz_target;

fn fuzz_dictionary() -> &'static WordList {
    static DICTIONARY: OnceLock<WordList> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        WordList::new(["analysis", "dictionary", "ferrolex", "source", "unicode"])
            .expect("the static fuzz dictionary is valid")
    })
}

fuzz_target!(|data: &[u8]| {
    let (mode, source) = data.split_first().unwrap_or((&0, &[]));
    let source = String::from_utf8_lossy(source);
    let comment_syntax = match mode % 3 {
        0 => CommentSyntax::None,
        1 => CommentSyntax::line("//"),
        _ => CommentSyntax::Html,
    };
    let document = Document::new(&source).with_comment_syntax(comment_syntax);
    let analysis = Analyzer::builder(fuzz_dictionary())
        .build()
        .check(&document);

    for finding in analysis.findings() {
        let _ = finding.whole_identifier_suggestion("replacement");
    }
});
