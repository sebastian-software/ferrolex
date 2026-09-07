# ferrolex

[![crates.io](https://img.shields.io/crates/v/ferrolex.svg)](https://crates.io/crates/ferrolex)
[![docs.rs](https://img.shields.io/docsrs/ferrolex)](https://docs.rs/ferrolex)
[![CI](https://github.com/sebastian-software/ferrolex/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sebastian-software/ferrolex/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-APACHE)
[![MSRV: 1.94](https://img.shields.io/badge/MSRV-1.94-blue.svg)](CONTRIBUTING.md#rust-toolchain-and-msrv)
[![Coverage gate: ≥ 79%](https://img.shields.io/badge/coverage%20gate-%E2%89%A5%2079%25-blue.svg)](.github/workflows/ci.yml)
[![Powered by Sebastian Software](https://img.shields.io/badge/Powered%20by-Sebastian%20Software-00718d?style=flat-square)](https://oss.sebastian-software.com)

Spell checking for text and code.

ferrolex is an independent Rust engine that safely loads existing Hunspell
dictionaries and provides fast, deterministic word checks and suggestions
without linking to the native Hunspell library. A verified dictionary catalog
and downloader cover the complete path from an upstream dictionary to a local,
caller-controlled cache.

The engine intentionally does not parse Markdown, programming languages, or
other document formats. Format-aware tools extract prose or identifiers and
call ferrolex through its Rust or Node.js API. This keeps language ownership in
projects designed for format-aware integrations, such as [ferromark for
Markdown](https://github.com/sebastian-software/ferromark), [ferrocat for PO
catalogs](https://github.com/sebastian-software/ferrocat), and [OXC for
TypeScript](https://oxc.rs), instead of turning ferrolex into a general
analysis framework. No sibling integration is part of the current support tier
yet; see [ADR-0010](docs/adr/0010-external-integration-support-tiers.md).

See the [documentation index](docs/README.md) for product contracts,
compatibility evidence, and retained prototype history.

## Status

The project is pre-1.0. The Rust engine, Hunspell compatibility, suggestions,
and managed dictionary acquisition are the current product focus. The Node.js
binding is the first direct runtime integration and requires Node.js 22.13 or
newer. Its `@ferrolex/node` package contract and eight-package prebuilt matrix
are release-gated in CI, but the package has not yet been published to npm.
The C ABI, Python, LSP, and VS Code implementations remain evaluation
prototypes without a PyPI, language-server, or extension release path. Public
APIs may change in minor releases before 1.0; breaking changes are recorded in
the changelog.

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

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the local validation gates,
commit conventions, release workflow, and provenance policy. The
[architecture overview](ARCHITECTURE.md) explains the product boundaries, and
the [ADR index](docs/adr/README.md) records the decisions behind them.

For a small first contribution, browse the
[open good-first issues](https://github.com/sebastian-software/ferrolex/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22).
Please read the linked context in each issue before changing a public contract
or compatibility boundary.

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

Create a UTF-8 word-list file with one word per line, then check either one
word or a plain-text file:

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
lines beginning with `#`. Exact matching, including casing, is the default for
plain word-list and compiled-dictionary checks; Hunspell imports apply
Hunspell-style capitalization fallback for initial-capital and all-uppercase
input. Library users can opt into NFC or NFKC normalization explicitly; case
folding remains a separate future policy. A complete tab-separated list in the form
`word<TAB>unsigned-frequency` is also accepted by `compile --dictionary` and
`check --dictionary`; the frequency is used for suggestion ranking and the
word portion is used for recognition. Directly loaded lists use only the word
portion, so frequency-ranked suggestions require the compiled artifact.
Format detection ignores blank lines and comments, including comments
containing tabs. A file with plain data or a trailing tab remains a plain word
list.

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
use ferrolex::{import, Dictionary, ImportMode, SuggestConfig};

let imported = import(
    "example.aff",
    "SET UTF-8\n",
    "example.dic",
    "1\nferrolex\n",
    ImportMode::Strict,
)?;
let dictionary = imported.dictionary();
assert!(dictionary.contains("ferrolex"));
assert_eq!(
    dictionary
        .suggester(SuggestConfig::default())
        .suggest("ferolex")
        .suggestions()[0]
        .word(),
    "ferrolex"
);
# Ok::<(), ferrolex::ImportError>(())
```

The `ferrolex` umbrella package re-exports the supported product crates as
`ferrolex::hunspell`, `ferrolex::suggest`, and `ferrolex::dictionaries`. The
common importer, dictionary, suggestion, and catalog types are also available
at the crate root. Depending only on `ferrolex` keeps these APIs on the same
version-locked release line.

The [Node.js binding](docs/bindings.md) is the supported pre-1.0 foreign-runtime
integration and remains in `default-members` because its Rust build is part of
the npm release gate, although the Cargo crate itself is `publish = false`.
The C ABI and Python binding are evaluation prototypes; the LSP and VS Code
work are retained editor prototypes outside the current product scope. These
prototype paths are excluded from the default release compilation and test
gates, then checked when their paths change or a maintainer starts the
prototype workflow manually. See [Native integrations](docs/integrations.md).

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

`compile --dictionary` turns the same word-list syntax used by `check` into a
deterministic native artifact. `compile <AFF> <DIC>` produces a
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
and the capabilities supported by the artifact format. For `FLXHSP`, this is
the format-wide reader contract rather than a claim about which optional
directives a particular source pair used. This gives release and locale-matrix
automation a stable, human-readable artifact report.

## Product boundaries

ferrolex owns dictionary acquisition, import, recognition, and suggestions. It
does not own document parsing, editor protocols, or language semantics. The
Node.js binding is the supported direct runtime integration; the C ABI and
Python binding are evaluation prototypes, while the LSP and VS Code work are
retained editor prototypes outside the product scope. Existing experimental
code may remain while the workspace is simplified, but it must not drive the
public API, release matrix, or future release gates. See
[ADR-0010](docs/adr/0010-external-integration-support-tiers.md) for the tier
definitions.

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

ferrolex supports the Rust version declared in the workspace
[`Cargo.toml`](Cargo.toml); see the [MSRV policy](CONTRIBUTING.md#rust-toolchain-and-msrv).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Dictionary data is not bundled with the engine and has separate licensing.

<!-- ferramenta-family:start -->
## The Ferramenta family

This project is part of [Ferramenta](https://ferramenta.dev) — the family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com) that keep the APIs the ecosystem already knows.

**The content pipeline**

| Tool | Job |
| --- | --- |
| [ferroni](https://sebastian-software.github.io/ferroni/) | Oniguruma-compatible regex engine |
| [ferriki](https://github.com/sebastian-software/ferriki) | Shiki-compatible syntax highlighting |
| [ferromark](https://sebastian-software.github.io/ferromark/) | Markdown to HTML — CommonMark & GFM |

**The language workshop**

| Tool | Job |
| --- | --- |
| **[ferrolex](https://github.com/sebastian-software/ferrolex)** | Spell checking for text and code |
| [ferrocat](https://ferrocat.dev) | Translation catalog engine |
| [palamedes](https://palamedes.dev) | Internationalization for TypeScript applications |

**On the workbench**

| Tool | Job |
| --- | --- |
| [ferrovia](https://github.com/sebastian-software/ferrovia) | SVGO-compatible SVG optimizer |
| [ferralk](https://github.com/sebastian-software/ferralk) | Glob matching and parallel filesystem walking |
| [ferrugo](https://github.com/sebastian-software/ferrugo) | PDF previews for untrusted files |
<!-- ferramenta-family:end -->

<!-- sebastian-software-branding:start -->

<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://sebastian-brand.vercel.app/sebastian-software/logo-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <strong>Built by Sebastian Software</strong> — consulting for TypeScript, React &amp; Rust.<br />
  <a href="https://sebastian-software.de">Work with us</a> · <a href="https://oss.sebastian-software.com">More open source</a>
</p>

<p align="center">Copyright &copy; 2026 Sebastian Software GmbH</p>

<!-- sebastian-software-branding:end -->
