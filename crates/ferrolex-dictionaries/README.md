# ferrolex-dictionaries

Verified acquisition of reviewed third-party Hunspell dictionaries for [ferrolex](https://github.com/sebastian-software/ferrolex).

The crate does not bundle dictionary data or update it silently. Select a
catalog entry, choose the cache root, and install its digest-verified files.

```rust,no_run
use ferrolex_dictionaries::{find_locale, DictionaryInstaller, UreqFetcher};

let source = find_locale("de_DE").expect("reviewed locale");
let manifest = source.manifest()?;
let installed = DictionaryInstaller::new(UreqFetcher)
    .install(&manifest, std::path::Path::new(".ferrolex-cache"))?;
println!("{}", installed.aff_path().display());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The caller owns cache placement and decides when acquisition occurs. See the
[API documentation](https://docs.rs/ferrolex-dictionaries) and the
[dictionary workflow](https://github.com/sebastian-software/ferrolex/blob/main/docs/dictionary-fetching.md).

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
