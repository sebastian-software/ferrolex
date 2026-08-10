# ferrolex

Modern spell-checking infrastructure for text and code.

ferrolex is an independent, native Rust implementation with planned support
for the Hunspell dictionary ecosystem. It is not a Hunspell port. The first
implementation slice provides immutable, UTF-8 plain-word-list dictionaries
and exact lookup; Hunspell import, morphology, suggestions, and source-code
analysis follow in later phases.

## Status

The project is in its initial development phase. The current public API and
CLI are intentionally small and may change before a stable release.

## Try it

Create a UTF-8 plain-word-list file with one word per line, then check either
one word or a plain-text file:

```sh
ferrolex check --dictionary words.txt Straße
ferrolex check --dictionary words.txt --file README.md
```

Plain-word-list files ignore blank lines, leading or trailing whitespace, and
lines beginning with `#`. Exact matching is the default. Library users can
opt into NFC or NFKC normalization explicitly; case folding remains a separate
future policy.

## Benchmarks

The core lookup benchmark is a local characterization harness, not a published
performance claim. Run it on a quiet machine with:

```sh
cargo bench -p ferrolex-core
```

See [Performance](docs/performance.md) for the measured contract.

## MSRV

ferrolex supports Rust 1.81 and later.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Dictionary data is not bundled with the engine and has separate licensing.
