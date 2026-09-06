# ferrolex-code

Generic source-code token and identifier analysis for [ferrolex](https://github.com/sebastian-software/ferrolex).

This supporting crate is language-agnostic. It classifies generic tokens,
handles identifier segments, and accepts caller-provided comment syntax. A
language adapter remains responsible for parsing its own source format.

```rust
use ferrolex_code::{Analyzer, Document};
use ferrolex_core::WordList;

let dictionary = WordList::new(["ferrolex"])?;
let analysis = Analyzer::builder(&dictionary)
    .build()
    .check(&Document::new("ferrolex typo"));
assert_eq!(analysis.findings().len(), 1);
# Ok::<(), ferrolex_core::WordListError>(())
```

See the [API documentation](https://docs.rs/ferrolex-code) and the
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
