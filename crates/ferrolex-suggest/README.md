# ferrolex-suggest

Deterministic, bounded spelling suggestions for [ferrolex](https://github.com/sebastian-software/ferrolex).

The suggestion engine works with any `CandidateSource`, applies explicit work
limits, and preserves deterministic ranking. Use a dictionary importer or a
core word list as the candidate source.

```rust
use ferrolex_core::WordList;
use ferrolex_suggest::{SuggestConfig, Suggester};

let dictionary = WordList::new(["ferrolex"])?;
let result = Suggester::new(&dictionary, SuggestConfig::default()).suggest("ferolex");
assert_eq!(result.suggestions()[0].word(), "ferrolex");
# Ok::<(), ferrolex_core::WordListError>(())
```

See the [API documentation](https://docs.rs/ferrolex-suggest) and the
[suggestion guide](https://github.com/sebastian-software/ferrolex/blob/main/docs/suggestions.md).

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
