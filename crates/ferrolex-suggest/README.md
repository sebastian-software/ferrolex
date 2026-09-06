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
