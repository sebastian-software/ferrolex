# Roadmap

## Phase 0: Repository Bootstrap

- Public GitHub repository.
- Rust workspace and crate layout.
- Core docs and ADRs.
- CI for format, check, tests, and clippy.

## Phase 1: Core Model And Tokenizer

- Stabilize `Span`, `Token`, `Finding`, `SourceAdapter`, `DictionaryProvider`,
  and `Checker`.
- Implement Unicode-aware tokenization.
- Implement identifier splitting for common code naming patterns.
- Add fixture tests for splitting behavior and token classification.

## Phase 2: Dictionary Import

- Define dictionary source manifest shape.
- Implement Hunspell source fetching.
- Parse enough `.aff` / `.dic` structure for initial languages.
- Compile source dictionaries into a Ferrolex-owned runtime format.
- Preserve source license metadata in generated manifests.

## Phase 3: Source Extraction

- Implement JSON string extraction through `ferrolex-json`.
- Implement JavaScript and TypeScript candidate extraction through OXC.
- Ignore programming-language keywords and structural syntax.
- Keep Palamedes-specific extraction out of v0.1.

## Phase 4: Lexios Brand Checks

- Consume Lexios Brand ID data as the canonical brand model.
- Compile Lexios lexicon entries into runtime checks.
- Emit findings for canonical spelling, forbidden variants, casing, and
  translation policy.
- Propose any missing brand semantics directly in Lexios.

## Phase 5: Integration Surface

- Add CLI or editor-facing layers only after the core API is useful.
- Revisit Node bindings once Rust APIs and compiled dictionary formats settle.
