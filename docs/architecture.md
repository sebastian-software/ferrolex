# Architecture

Ferrolex is organized as a small validation pipeline. Each stage should stay
independently usable so downstream tools can adopt only the pieces they need.

## Pipeline

1. Source adapters produce `Span` values from input files or direct text.
2. The tokenizer turns spans into `Token` values.
3. Dictionary providers check token membership and provide suggestions.
4. Lexios-backed brand rules check canonical terms, forbidden variants, casing,
   and translation guidance.
5. Checkers produce `Finding` values with stable rule ids, severities, source
   ranges, and suggestions.

## Core Types

`ferrolex-core` owns the shared vocabulary:

- `Span`: text plus optional source path, byte range, locale, and role.
- `Token`: original and normalized text plus token kind and range.
- `Finding`: structured diagnostic emitted by a checker.
- `DictionaryProvider`: dictionary lookup and suggestion trait.
- `SourceAdapter`: extraction trait for JSON, OXC, and future source adapters.
- `Checker`: validation trait over spans and tokens.

The core crate must not depend on concrete parsers, dictionary formats, Lexios
implementation details, or CLI/editor concerns.

## Source Adapters

The first source adapters are intentionally narrow:

- `ferrolex-json` should extract string values from JSON-like localization and
  configuration documents.
- `ferrolex-oxc` should extract candidate strings from JavaScript and
  TypeScript using OXC. It should ignore official programming-language syntax
  such as `const`, `function`, and keywords, and it should avoid
  Palamedes-specific conventions.

Future adapters can support PO, NDJSON, YAML, Markdown, or framework-specific
sources without changing the checker core.

## Tokenization

`ferrolex-tokenizer` is a core subsystem, not an adapter detail. It should own:

- Unicode-aware word boundaries.
- Identifier splitting for `camelCase`, `PascalCase`, `snake_case`,
  `kebab-case`, numeric boundaries, and acronym boundaries.
- Early classification of URLs, hashes, UUIDs, color values, generated ids,
  technical placeholders, and other tokens that should usually not hit a
  spelling dictionary.

Tokenizer behavior should be deterministic, fixture-driven, and reusable by all
source adapters.

## Validation

Dictionary validation and brand validation are separate layers:

- Dictionary validation answers whether a token is accepted for a locale.
- Brand validation answers whether text follows Lexios-defined terminology,
  casing, forbidden variants, translation policy, and brand-specific rules.

Both layers emit `Finding` values so callers can combine, filter, sort, or
render diagnostics consistently.
