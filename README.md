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

## MSRV

ferrolex supports Rust 1.70 and later.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Dictionary data is not bundled with the engine and has separate licensing.
