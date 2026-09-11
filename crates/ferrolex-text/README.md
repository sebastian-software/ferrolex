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

<!-- ferramenta-family:start -->
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — A family of Rust tools.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) — Oniguruma-compatible regex engine · [ferriki](https://github.com/sebastian-software/ferriki) — Shiki-compatible syntax highlighting · [ferromark](https://sebastian-software.github.io/ferromark/) — Markdown to HTML with a secure default and every GFM extension included. · [ferrocat](https://ferrocat.dev) — Translation catalog engine · [palamedes](https://palamedes.dev) — Internationalization for TypeScript applications · [ferrovia](https://github.com/sebastian-software/ferrovia) — SVGO-compatible SVG optimizer · [ferralk](https://github.com/sebastian-software/ferralk) — Glob matching and parallel filesystem walking · [ferrugo](https://github.com/sebastian-software/ferrugo) — PDF previews for untrusted files.
<!-- ferramenta-family:end -->

<!-- sebastian-software-branding:start -->

<p align="center">
  <a href="https://oss.sebastian-software.com"><img src="https://sebastian-brand.vercel.app/sebastian-software/logo-software.svg" alt="Sebastian Software" width="160" /></a><br />
  TypeScript, React &amp; Rust consulting<br />
  Experts in Agentic Software Development<br />
  <a href="https://sebastian-software.de">Work with us</a> · <a href="https://oss.sebastian-software.com">More open source</a>
</p>

<p align="center">Copyright &copy; 2026 Sebastian Software GmbH</p>

<!-- sebastian-software-branding:end -->
