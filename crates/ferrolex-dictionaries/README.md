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
