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
