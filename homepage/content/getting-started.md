# Getting started

Ferrolex checks words against dictionaries and returns deterministic suggestions.
It is pre-1.0: public APIs can change in minor releases, and Hunspell support has
explicit compatibility boundaries. Start with the Rust library or command-line tool.

## Install the command-line tool

```sh
cargo install --locked ferrolex-cli
```

Create a UTF-8 file named `words.txt`, with one word per line:

```text
ferrolex
Ferris
```

Check a word or request suggestions:

```sh
ferrolex check --dictionary words.txt ferrolex
ferrolex suggest --dictionary words.txt ferolex
```

The [engine and CLI reference](../../README.md.src#try-it) covers plain text,
Hunspell sources and compiled artifacts.

## Use the Rust library

```sh
cargo add ferrolex
```

The [Rust quick start](../../README.md.src#rust-library-quick-start) shows dictionary
import and suggestions. The published API documentation is available on
[docs.rs](https://docs.rs/ferrolex).

## Choose dictionary data explicitly

Ferrolex does not bundle dictionary data. The reviewed catalog downloads pinned
sources into a cache you choose; ordinary checks never download or update data
implicitly. Follow the [dictionary acquisition guide](../../docs/dictionary-fetching.md)
and check the [locale compatibility evidence](../../docs/locale-compatibility.md)
for the exact source pair you intend to use.

## Integrate at the word boundary

Your application extracts words from its document or language format. Ferrolex
owns dictionary import, recognition and suggestions. The
[product-boundary decision](../../docs/adr/0009-language-aware-source-analysis.md)
explains why document parsing stays with format-aware tools.

The Node.js binding is implemented but not yet published to npm. C ABI, Python,
LSP and editor work remain prototypes; see the [support tiers](../../docs/adr/0010-external-integration-support-tiers.md).
