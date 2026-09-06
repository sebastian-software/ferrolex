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
