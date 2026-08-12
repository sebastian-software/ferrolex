# Hunspell lookup explanations

`HunspellDictionary::explain` is the allocating diagnostic companion to
`Dictionary::contains`. It answers why a word was accepted or rejected without
changing the ordinary recognition path.

## Contract

The normal `contains` lookup remains the allocation-free, bounded hot path.
`explain` intentionally replays bounded derivation and compound search to
return owned diagnostic data. It is appropriate for a CLI, editor inspection,
or troubleshooting UI; it is not intended for every token in a document scan.

The public result distinguishes:

- an accepted stored stem;
- an affixed form with the applied prefix/suffix chain and continuation flags;
- a compound with its matched stored components and their generic or positioned
  roles; and
- a rejected form with `FORBIDDENWORD`, `NEEDAFFIX`, `ONLYINCOMPOUND`,
  `KEEPCASE`, or no viable derivation as the available reason.

It also records whether the direct spelling or Hunspell's capitalization
fallback accepted the word.

```rust
use ferrolex_hunspell::{import, AcceptanceKind, ImportMode};

let dictionary = import(
    "example.aff",
    "SFX A Y 1\nSFX A 0 s .\n",
    "example.dic",
    "1\nword/A\n",
    ImportMode::Strict,
)?.dictionary().clone();

let explanation = dictionary.explain("words");
let accepted = explanation.accepted().expect("the word is accepted");
assert!(matches!(accepted.kind(), AcceptanceKind::Affixed { stem, .. } if stem == "word"));
# Ok::<(), ferrolex_hunspell::ImportError>(())
```

## Stability and limits

This is an **experimental diagnostic API**. Its result types are intentionally
`#[non_exhaustive]`; consumers should keep wildcard branches when matching
them. The returned path is deterministic for the currently supported bounded
Hunspell semantics, but is not a serialized interchange format or a promise to
expose every private search implementation detail forever.

The existing Criterion `hunspell lookup` group measures the direct `contains`
lanes (`hit`, `miss`, `affixed`, `compound`, and `mixed-case`). It is the
regression guard for this split: explanation work must remain outside that
hot-path benchmark.
