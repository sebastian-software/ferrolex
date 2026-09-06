# ferrolex-hunspell

Hunspell-compatible dictionary import for [ferrolex](https://github.com/sebastian-software/ferrolex).

The importer accepts the documented supported subset of textual Hunspell AFF
and DIC files, reports source-aware diagnostics, and produces a dictionary with
recognition and suggestion metadata.

```rust
use ferrolex_core::Dictionary;
use ferrolex_hunspell::{import, ImportMode};

let imported = import(
    "example.aff",
    "SET UTF-8\n",
    "example.dic",
    "1\nferrolex\n",
    ImportMode::Strict,
)?;
assert!(imported.dictionary().contains("ferrolex"));
# Ok::<(), ferrolex_hunspell::ImportError>(())
```

See the [API documentation](https://docs.rs/ferrolex-hunspell), the
[Hunspell format guide](https://github.com/sebastian-software/ferrolex/blob/main/docs/hunspell-format.md),
and the [compatibility notes](https://github.com/sebastian-software/ferrolex/blob/main/docs/locale-compatibility.md).

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
