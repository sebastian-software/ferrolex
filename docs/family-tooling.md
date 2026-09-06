# Shared family tooling

ferrolex keeps format ownership in the consuming tool, but the family shares a
small set of stable, format-neutral building blocks. This page records the
boundary so integrations do not each grow a slightly different tokenizer or
CLI contract.

## `ferrolex-text` is the plain-text tokenizer

[`ferrolex-text`](https://docs.rs/ferrolex-text) is the canonical tokenizer for
plain text and extracted catalog strings. `check_text` returns the original
token and its UTF-8 byte range while applying the shared rules for Unicode
letters, combining marks, apostrophes, and alphanumeric adjacency. Recognition
is delegated to the caller-provided `ferrolex-core::Dictionary`.

The crate deliberately does not parse Markdown, PO, source code, or any other
format. A consuming tool remains responsible for selecting fields, preserving
file/catalog coordinates, and applying its own ignore policy before passing a
string to `check_text`.

```rust
use ferrolex_core::WordList;
use ferrolex_text::check_text;

let dictionary = WordList::new(["known"])?;
let findings = check_text(&dictionary, "known typo").collect::<Vec<_>>();
assert_eq!(findings[0].word(), "typo");
# Ok::<(), ferrolex_core::WordListError>(())
```

## Evaluation queue

The first consumers to evaluate the shared tokenizer are:

- Palamedes and ferrocat: spell-check extracted PO/catalog strings while
  retaining the catalog entry and source-field location.
- Ferramenta: use the CLI's text-checking contract for the documentation check,
  with the repository's own file selection and ignore policy.

These are integration evaluations, not new ferrolex format parsers. Each
consumer should record its selected fields, dictionary source, ignored-token
policy, and byte-to-location mapping in its own tests before adopting the
crate as a production dependency.

## Other shared tooling

The CLI argument boundary uses `clap` while the public command model remains
owned by `ferrolex-cli`. Release ordering and the Cargo/Node/Python/VS Code
version invariant remain enforced by the checked-in release scripts until a
family release action accepts those contracts directly. Filesystem walking is
still intentionally separate: adopting `ferralk` requires a compatible
published version or an explicit workspace MSRV decision.
