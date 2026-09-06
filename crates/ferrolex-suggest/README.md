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
