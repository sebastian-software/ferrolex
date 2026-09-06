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
