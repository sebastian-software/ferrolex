# ferrolex

Modern spell-checking infrastructure for text and code.

ferrolex is an independent, native Rust implementation with planned support
for the Hunspell dictionary ecosystem. It is not a Hunspell port. It currently
provides immutable UTF-8 plain-word-list dictionaries, generic source-code
analysis, bounded deterministic suggestions, and a documented Hunspell
`.aff`/`.dic` recognition subset. That subset includes lazy affixes,
continuations, circumfixes, special-word flags, and bounded compound rules;
unsupported directives remain explicit diagnostics rather than compatibility
claims.

## Status

The project is in its initial development phase. The current public API and
CLI are intentionally small and may change before a stable release.

## Try it

Create a UTF-8 plain-word-list file with one word per line, then check either
one word or a plain-text file:

```sh
ferrolex check --dictionary words.txt Straße
ferrolex check --dictionary words.txt --file README.md
ferrolex suggest --dictionary words.txt Strase
ferrolex analyze --dictionary words.txt --comment-prefix // src/lib.rs
ferrolex validate --strict dictionary.aff dictionary.dic
ferrolex compile --dictionary words.txt -o words.flex
ferrolex validate --compiled words.flex
ferrolex check --compiled words.flex Straße
ferrolex dictionary list
ferrolex dictionary install pl_PL --cache .ferrolex-dictionaries
ferrolex check --hunspell .ferrolex-dictionaries/pl_PL/pl_PL.aff słowami
ferrolex analyze --hunspell .ferrolex-dictionaries/pl_PL/pl_PL.aff src/lib.rs
ferrolex suggest --hunspell .ferrolex-dictionaries/pl_PL/pl_PL.aff --max-candidates 300000 --max-edit-cells 20000000 slowami
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

`validate` imports a Hunspell-style pair under ferrolex's documented
compatibility subset and reports structured diagnostics. It decodes UTF-8,
ISO-8859-1, and ISO-8859-2 source pairs from their `SET` declaration; reviewed
mixed-encoding catalog pairs are handled by `dictionary install`. It never
invokes an external spell-checking engine; see the
[import contract](docs/hunspell-format.md) and
[affix semantics](docs/affix-semantics.md).

`suggest` exposes bounded, deterministic edit-distance suggestions for one
plain-word-list dictionary or one installed Hunspell runtime cache. It reports
when its configured work limits prevent a complete search, but still returns
any stable partial results. Hunspell suggestions use stored stems only and do
not expand affix or compound forms. `UserDictionary` project overlays can be
used through the library API. The comparison and ranking contract is documented
in [Suggestions](docs/suggestions.md).

The current native integration boundary and explicit LSP/FFI deferral are
recorded in [Native integrations](docs/integrations.md).

The optional, digest-pinned LibreOffice installer is documented in
[Dictionary fetching](docs/dictionary-fetching.md). It provides reviewed
LibreOffice sources for English, German, Spanish, French, Italian, Portuguese,
Dutch, Polish, Russian, Turkish, Arabic, Ukrainian, Swedish, Indonesian,
Hindi, and Bengali; it never downloads or updates dictionaries implicitly.
Urdu requires a separately reviewed source because it has no pair in the
pinned LibreOffice collection. CJK is intentionally deferred until text
segmentation has its own contract. The
[locale compatibility matrix](docs/locale-compatibility.md) separates safe
acquisition from strict import and recognition evidence. Successful strict
installs also create a [versioned Hunspell runtime cache](docs/hunspell-runtime-cache.md).

`compile` turns the same plain-word-list syntax used by `check` into a
deterministic native artifact. `check --compiled` uses its allocation-free
exact lookup directly and can be layered with plain or installed Hunspell
dictionaries. `validate --compiled` first performs the fast header/checksum
load and then fully validates every offset, UTF-8 payload, and sort-order
invariant. The [binary format](docs/binary-format.md) documents the artifact
layout and compatibility policy.

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
