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

<!-- ferramenta-family:registry:start -->
Part of the [Ferramenta](https://ferramenta.dev) family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com).
Siblings: [ferroni](https://github.com/sebastian-software/ferroni), [ferriki](https://github.com/sebastian-software/ferriki), [ferromark](https://github.com/sebastian-software/ferromark), [ferralk](https://github.com/sebastian-software/ferralk), [ferrovia](https://github.com/sebastian-software/ferrovia), [ferrocat](https://github.com/sebastian-software/ferrocat), and [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:registry:end -->

<!-- sebastian-software-branding:start -->
<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://raw.githubusercontent.com/sebastian-software/ferramenta/main/app/assets/logos/sebastian-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <a href="https://oss.sebastian-software.com">Open Source at Sebastian Software</a><br />
  Copyright &copy; 2026 Sebastian Software GmbH
</p>
<!-- sebastian-software-branding:end -->
