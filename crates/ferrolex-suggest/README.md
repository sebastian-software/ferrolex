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
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — Rust-native developer tools that keep the APIs the ecosystem already knows.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) · [ferriki](https://github.com/sebastian-software/ferriki) · [ferromark](https://sebastian-software.github.io/ferromark/) · [ferrocat](https://ferrocat.dev) · [palamedes](https://palamedes.dev) · [ferrovia](https://github.com/sebastian-software/ferrovia) · [ferralk](https://github.com/sebastian-software/ferralk) · [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:end -->

<!-- sebastian-software-branding:start -->

<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://sebastian-brand.vercel.app/sebastian-software/logo-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <strong>Built by Sebastian Software</strong> — consulting for TypeScript, React &amp; Rust.<br />
  <a href="https://sebastian-software.de">Work with us</a> · <a href="https://oss.sebastian-software.com">More open source</a>
</p>

<p align="center">Copyright &copy; 2026 Sebastian Software GmbH</p>

<!-- sebastian-software-branding:end -->
