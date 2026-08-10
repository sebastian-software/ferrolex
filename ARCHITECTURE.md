# Architecture

ferrolex keeps input formats, word recognition, document analysis, suggestions,
and compiled storage separate so that no importer or analyzer becomes the core
runtime model.

```text
plain word list / Hunspell / compiled dictionary
                    │
                    ▼
             ferrolex-core
                    │
          ┌─────────┴──────────┐
          ▼                    ▼
    ferrolex-text         ferrolex-code
          │                    │
          └─────────┬──────────┘
                    ▼
                ferrolex CLI
```

## Current crates

- `ferrolex-core` owns immutable dictionary lookup, explicit normalization,
  and dictionary composition. Its public `Dictionary` trait is intentionally
  small; importers and future binary loaders implement it at their boundary.
- `ferrolex-text` owns natural-language tokenization. It returns original token
  slices and UTF-8 byte ranges, without encoding file-format semantics in the
  dictionary layer.
- `ferrolex-code` owns generic source token classification, identifier
  segmentation, inline directives, and diagnostics. It remains parser- and
  language-agnostic; future adapters supply document and comment context.
- `ferrolex-cli` owns process I/O, exit codes, and rendering diagnostics.

Base dictionaries are immutable, `Send`, and `Sync`. A mutable user overlay,
Hunspell import, morphology, source-code analysis, suggestions, and the native
compiled format are separate forthcoming layers.

## Normalization boundary

`Normalization::Exact` preserves the supplied UTF-8 form. `Nfc` and `Nfkc`
transform both dictionary entries and lookup queries before comparison. None of
these options case-fold: casing is language-sensitive and will receive its own
contract rather than being silently coupled to Unicode normalization.
