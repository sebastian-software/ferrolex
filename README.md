# ferrolex

Native spell-checking for Rust and Node.js.

ferrolex is an independent Rust engine that safely loads existing Hunspell
dictionaries and provides fast, deterministic word checks and suggestions
without linking to the native Hunspell library. A verified dictionary catalog
and downloader cover the complete path from an upstream dictionary to a local,
caller-controlled cache.

The engine intentionally does not parse Markdown, programming languages, or
other document formats. Format-aware tools extract prose or identifiers and
call ferrolex through its Rust or Node.js API. This keeps language ownership in
projects such as Ferromark for Markdown, Ferrocat for PO catalogs, and OXC for
TypeScript instead of turning ferrolex into a general analysis framework.

See the [documentation index](docs/README.md) for product contracts,
compatibility evidence, and retained prototype history.

## Status

The project is pre-1.0. The Rust engine, Hunspell compatibility, suggestions,
and managed dictionary acquisition are the current product focus. The Node.js
binding is the first direct runtime integration and remains unpublished while
its package and installation contract are completed. Public APIs may change in
minor releases before 1.0; breaking changes are recorded in the changelog.

### Reviewed dictionary compatibility

This concise status is generated from the digest-pinned real-world fixture
catalog. CI checks it in the relevant-change compatibility gate and in the
weekly, manual, and release differential scorecard runs.

- ✅ **Ready for the tested core**: the exact pinned dictionary imports without
  recognition errors and its reviewed word forms work.
- 🟡 **In progress**: pinned probes and cache roundtrips pass, while the exact
  strict-import blockers remain review-gated.
- 🔴 **Blocked**: ferrolex cannot reliably import that exact dictionary yet.

This is deliberately not a “100% Hunspell compatible” claim. The [full locale
matrix](docs/locale-compatibility.md) records the boundaries, and the oracle
scorecard artifact contains the current differential evidence.

<!-- compat-status:start -->
| Dictionary locale | Status | What this means |
| --- | --- | --- |
| `en_US` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `de_DE` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `es_ES` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `fr_FR` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `it_IT` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `pt_BR` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `pt_PT` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `nl_NL` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `hu_HU` | 🟡 In progress | Pinned probes and cache roundtrips pass; exact strict-import blockers are review-gated. |
| `ar` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `tr_TR` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
| `pl_PL` | ✅ Ready for the tested core | The pinned dictionary imports strictly and its reviewed word forms work. |
<!-- compat-status:end -->

## Install

Install the command-line tool from crates.io, or build it from a checkout:

```sh
cargo install ferrolex-cli
# or, from this repository:
cargo build -p ferrolex-cli
```

The build places the binary at `target/debug/ferrolex`; `cargo install` adds
`ferrolex` to Cargo's bin directory.

## Try it

Create a UTF-8 plain-word-list file with one word per line, then check either
one word or a plain-text file:

```sh
ferrolex check --dictionary words.txt Straße
ferrolex check --dictionary words.txt --file README.md
ferrolex check --dictionary words.txt --file README.md CHANGELOG.md
printf 'text from stdin' | ferrolex check --dictionary words.txt --file -
ferrolex check --dictionary words.txt -- --hyphenated-word
ferrolex check --format json --dictionary words.txt --file README.md
ferrolex suggest --dictionary words.txt Strase
ferrolex validate --strict dictionary.aff dictionary.dic
ferrolex check --hunspell dictionary.aff derived-form
ferrolex dictionary list
ferrolex dictionary install pl_PL --cache .ferrolex-dictionaries
ferrolex check --hunspell .ferrolex-dictionaries/pl_PL/pl_PL.aff słowami
ferrolex suggest --hunspell .ferrolex-dictionaries/pl_PL/pl_PL.aff slowami
```

Plain-word-list files ignore blank lines, leading or trailing whitespace, and
lines beginning with `#`. Exact matching is the default. Library users can
opt into NFC or NFKC normalization explicitly; case folding remains a separate
future policy.

`validate` imports a Hunspell-style pair under ferrolex's documented
compatibility subset and reports structured diagnostics. It decodes UTF-8,
ISO-8859-1, and ISO-8859-2 source pairs from their `SET` declaration; reviewed
mixed-encoding catalog pairs are handled by `dictionary install`. It never
invokes an external spell-checking engine; see the
[import contract](docs/hunspell-format.md) and
[affix semantics](docs/affix-semantics.md).

`--hunspell` accepts an ordinary `.aff` path and derives the adjacent `.dic`.
It verifies and uses an installed runtime cache when present; otherwise it
strictly imports the sources with a slower-path notice and does not write next
to them. Importer errors fail closed. For frequent use or read-only source
directories, compile the pair to a writable standalone artifact and pass it
with `--compiled`; catalog-specific encoding overrides require `dictionary
install` and are never inferred from a filename alone.

`suggest` exposes bounded, deterministic edit-distance suggestions across any
number of layered plain-word-list dictionaries, installed Hunspell runtime
caches, and compiled artifacts. Each source flag is repeatable, just as it is
for `check` and `analyze`. It reports when its configured work limits prevent a
complete search, still returns any stable partial results, and prints a scaled
retry hint when budget exhaustion produced no result. Hunspell
suggestions enumerate stored stems and additionally derive bounded affixed and
compound forms near the query; they never pre-expand the dictionary.
`UserDictionary` project overlays can be used through the library API. The CLI
automatically layers `.ferrolex/words.txt` and the global ferrolex user word
list into `check`, `suggest`, and `analyze` when those files exist. The
comparison and ranking contract is documented in [Suggestions](docs/suggestions.md).

## Rust library quick start

```rust
use ferrolex::{Dictionary, WordList};

let dictionary = WordList::new(["ferrolex"])?;
assert!(dictionary.contains("ferrolex"));
# Ok::<(), ferrolex::WordListError>(())
```

The [Node.js binding](docs/bindings.md) is the only current foreign-runtime
integration direction. The checked-in C ABI, Python, LSP, and VS Code work is
prototype history rather than an active distribution or compatibility promise;
it is excluded from the default release compilation and test gates, then
checked when prototype paths change or a maintainer starts the prototype
workflow manually. See [Native integrations](docs/integrations.md).

The optional, digest-pinned LibreOffice installer is documented in
[Dictionary fetching](docs/dictionary-fetching.md). It fetches reviewed
upstream sources into a cache you select; ferrolex neither bundles nor
redistributes dictionary content, and normal commands never download or update
dictionaries implicitly. The catalog provides a reviewed per-locale SPDX
expression and upstream notice for English, German, Spanish, French, Italian,
Portuguese, Dutch, Polish, Russian, Turkish, Arabic, Ukrainian, Swedish,
Indonesian, Hindi, and Bengali.
Urdu requires a separately reviewed source because it has no pair in the
pinned LibreOffice collection. CJK is intentionally deferred until text
segmentation has its own contract. The
[locale compatibility matrix](docs/locale-compatibility.md) separates safe
acquisition from strict import and recognition evidence. Successful strict
installs also create a [versioned Hunspell runtime cache](docs/hunspell-runtime-cache.md).

`compile --dictionary` turns the same plain-word-list syntax used by `check`
into a deterministic native artifact. `compile <AFF> <DIC>` produces a
standalone Hunspell artifact that retains ferrolex's supported affix semantics
and can be copied to a machine without the source pair. `check --compiled`
loads either artifact type and can be layered with plain or installed Hunspell
dictionaries. `validate --compiled` verifies its format before use; native
artifacts additionally receive the full offset, UTF-8 payload, and sort-order
check. The [binary format](docs/binary-format.md) and
[Hunspell runtime cache](docs/hunspell-runtime-cache.md) document the formats
and compatibility policy.

`inspect` makes the compatibility boundary visible before deployment. It prints
the artifact format and version, source metadata where the format records it,
and the recognition features the reader must support. This gives release and
locale-matrix automation a stable, human-readable artifact report.

## Product boundaries

ferrolex owns dictionary acquisition, import, recognition, and suggestions. It
does not own document parsing, editor protocols, or language semantics. In
particular, parser-backed source analysis, an LSP, and editor extensions are not
part of the current product scope. Existing experimental code may remain while
the workspace is simplified, but it must not drive the public API, release
matrix, or future release gates. Some prototype checks still run in the general
workspace CI during this transition and are tracked for removal from the
focused release path.

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

## Security

Please report vulnerabilities privately as described in the
[security policy](SECURITY.md). Dictionary and artifact inputs are treated as
untrusted throughout the supported import and loading paths.

## MSRV

ferrolex supports Rust 1.88 and later.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Dictionary data is not bundled with the engine and has separate licensing.
