# ferrolex-text

Plain-text tokenization and spell-checking helpers for [ferrolex](https://github.com/sebastian-software/ferrolex).

This supporting crate finds natural-language words and delegates recognition to
a `ferrolex-core::Dictionary`. It does not parse Markdown, source code, or
other document formats.

It is the family tokenizer for plain text and strings extracted by
format-aware consumers such as catalog and documentation tools. Consumers own
field selection, source locations, and ignore policy; `ferrolex-text` owns the
shared Unicode token and byte-range contract.

```rust
use ferrolex_core::WordList;
use ferrolex_text::check_text;

let dictionary = WordList::new(["known"])?;
assert_eq!(check_text(&dictionary, "known typo").count(), 1);
# Ok::<(), ferrolex_core::WordListError>(())
```

See the [API documentation](https://docs.rs/ferrolex-text) and the
[workspace architecture](https://github.com/sebastian-software/ferrolex/blob/main/ARCHITECTURE.md).
