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
