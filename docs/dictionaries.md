# Dictionaries

Ferrolex should treat Hunspell-compatible dictionaries as source material and
compile them into a Ferrolex-owned runtime representation.

## Why Hunspell First

Hunspell remains the practical starting point for v0.1:

- Broad multilingual dictionary availability.
- Existing `.aff` / `.dic` source format used by office suites, browsers, and
  spellchecking tools.
- Better ecosystem coverage than most standalone alternatives.

The important boundary is runtime ownership. Ferrolex should not require callers
to reason about Hunspell internals during checking. Import and conversion can be
Hunspell-aware; lookup should go through Ferrolex APIs.

## Alternatives

- Nuspell is a modern spellchecker implementation, but it primarily consumes
  Hunspell/MySpell dictionaries.
- Enchant is a provider abstraction rather than a dictionary source.
- Aspell is a real alternative, but it is older and less aligned with current
  application packaging.
- LanguageTool and Morfologik are valuable for grammar, style, and Java-based
  language tooling, but they are not the right first runtime boundary for a
  small Rust dictionary library.
- Voikko, Hspell, Zemberek, and similar tools matter for specific languages but
  do not solve the general multilingual dictionary packaging problem.

## Planned Flow

1. Maintain an allowlist of approved dictionary sources.
2. Fetch source archives or files with pinned versions or checksums.
3. Record source URL, upstream version, locale, license, and redistribution
   constraints.
4. Import `.aff` and `.dic` files.
5. Compile dictionary data into a Ferrolex-owned runtime format.
6. Load compiled dictionaries through `ferrolex-dict`.

## License Metadata

Dictionary packages must keep per-language license metadata explicit. The Rust
crates can be MIT while individual dictionary data may carry separate licenses.
The downloader/converter should preserve this distinction and make it visible in
generated manifests.

## Runtime Format Direction

The runtime dictionary format should optimize for:

- Fast membership checks.
- Compact storage.
- Memory-mappable or cheap-to-load artifacts.
- Locale-specific metadata.
- Future suggestion ranking.
- Separation between known typo maps and full word dictionaries.

The exact format is intentionally not selected during the bootstrap. FST-based
sets and related compact lookup structures are strong candidates once real
fixtures exist.
