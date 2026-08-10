# ferrolex

Modern spell-checking infrastructure for text and code.

ferrolex is an independent, native Rust implementation with planned support
for the Hunspell dictionary ecosystem. It is not a Hunspell port. It currently
provides immutable UTF-8 plain-word-list dictionaries, generic source-code
analysis, and a documented basic Hunspell `.aff`/`.dic` import subset;
advanced morphology and suggestions follow in later phases.

## Status

The project is in its initial development phase. The current public API and
CLI are intentionally small and may change before a stable release.

## Try it

Create a UTF-8 plain-word-list file with one word per line, then check either
one word or a plain-text file:

```sh
ferrolex check --dictionary words.txt Straße
ferrolex check --dictionary words.txt --file README.md
ferrolex analyze --dictionary words.txt --comment-prefix // src/lib.rs
ferrolex validate --strict dictionary.aff dictionary.dic
ferrolex compile --dictionary words.txt -o words.flex
ferrolex validate --compiled words.flex
ferrolex dictionary list
ferrolex dictionary fetch pt_BR --cache .ferrolex-dictionaries
```

Plain-word-list files ignore blank lines, leading or trailing whitespace, and
lines beginning with `#`. Exact matching is the default. Library users can
opt into NFC or NFKC normalization explicitly; case folding remains a separate
future policy.

`analyze` is the generic source-code path. It splits camelCase, PascalCase,
snake_case, kebab-case, and Unicode identifiers; ignores URLs, email addresses,
numbers, and hashes by default; and recognizes `ferrolex:ignore`,
`ferrolex:disable`, and `ferrolex:enable` only inside the declared comment
syntax. See [source-code analysis](docs/source-code-analysis.md).

`validate` imports a UTF-8 Hunspell-style pair under ferrolex's documented
compatibility subset and reports structured diagnostics. It never invokes an
external spell-checking engine; see the [import contract](docs/hunspell-format.md)
and [affix semantics](docs/affix-semantics.md).

The `ferrolex-suggest` library crate provides bounded, deterministic
edit-distance suggestions for enumerable word sources. Its comparison and
ranking contract is documented in [Suggestions](docs/suggestions.md).

The current native integration boundary and explicit LSP/FFI deferral are
recorded in [Native integrations](docs/integrations.md).

The optional, digest-pinned LibreOffice installer is documented in
[Dictionary fetching](docs/dictionary-fetching.md). It provides reviewed
LibreOffice sources for English, German, Spanish, French, Italian, Portuguese,
Dutch, Polish, Russian, Turkish, Arabic, Ukrainian, Swedish, Indonesian,
Hindi, and Bengali; it never downloads or updates dictionaries implicitly.
Urdu requires a separately reviewed source because it has no pair in the
pinned LibreOffice collection. CJK is intentionally deferred until text
segmentation has its own contract.

`compile` turns the same plain-word-list syntax used by `check` into a
deterministic native artifact. `validate --compiled` first performs the fast
header/checksum load and then fully validates every offset, UTF-8 payload, and
sort-order invariant. The [binary format](docs/binary-format.md) documents the
artifact layout and compatibility policy.

## Benchmarks

The core lookup benchmark is a local characterization harness, not a published
performance claim. Run it on a quiet machine with:

```sh
cargo bench -p ferrolex-core
```

See [Performance](docs/performance.md) for the measured contract.

## Robustness testing

The regular test suite contains deterministic adversarial corpora for
untrusted Hunspell input, compiled artifacts, and bounded suggestions. See
[Robustness testing](docs/robustness-testing.md) for the covered boundaries
and focused command.

## MSRV

ferrolex supports Rust 1.81 and later.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Dictionary data is not bundled with the engine and has separate licensing.
