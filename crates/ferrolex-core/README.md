# ferrolex-core

Core dictionary traits and in-memory dictionary layers for [ferrolex](https://github.com/sebastian-software/ferrolex).

This supporting crate defines `Dictionary`, `CandidateSource`, normalization,
`WordList`, `UserDictionary`, and `Checker`. Format-specific importers and
compiled loaders implement these interfaces at their own boundaries.

```rust
use ferrolex_core::{Dictionary, WordList};

let dictionary = WordList::new(["ferrolex"])?;
assert!(dictionary.contains("ferrolex"));
# Ok::<(), ferrolex_core::WordListError>(())
```

See the [API documentation](https://docs.rs/ferrolex-core) and the
[workspace architecture](https://github.com/sebastian-software/ferrolex/blob/main/ARCHITECTURE.md).

<!-- ferramenta-family:registry:start -->
Part of the [Ferramenta](https://ferramenta.dev) family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com).
Siblings: [ferroni](https://github.com/sebastian-software/ferroni), [ferriki](https://github.com/sebastian-software/ferriki), [ferromark](https://github.com/sebastian-software/ferromark), [ferralk](https://github.com/sebastian-software/ferralk), [ferrovia](https://github.com/sebastian-software/ferrovia), [ferrocat](https://github.com/sebastian-software/ferrocat), and [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:registry:end -->

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
