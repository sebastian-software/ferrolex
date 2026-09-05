# Architecture

ferrolex is a spell-checking engine, not a document-analysis framework. Its
core boundary starts with dictionary bytes and ends with deterministic word
recognition and suggestions.

```text
reviewed upstream sources / local Hunspell pairs / plain word lists
                              │
                              ▼
                 ferrolex-dictionaries
                    verified fetch/cache
                              │
                              ▼
                    ferrolex-hunspell
                  import and runtime cache
                              │
                              ▼
                      ferrolex-core
                 immutable Dictionary contract
                              │
                   ┌──────────┴──────────┐
                   ▼                     ▼
          ferrolex-suggest        reference CLI
                   │
                   ▼
             Rust consumers
                   │
                   ▼
           ferrolex-node adapter
```

## Product crates

- `ferrolex-core` owns immutable dictionary lookup, explicit normalization,
  composition, and user overlays. Its `Dictionary` trait is the central
  integration contract.
- `ferrolex-hunspell` safely imports supported `.aff`/`.dic` semantics and owns
  the provenance-bound runtime cache.
- `ferrolex-suggest` owns bounded, deterministic suggestions without changing
  dictionary recognition semantics.
- `ferrolex-dictionaries` owns the reviewed catalog, verified acquisition, and
  caller-selected local cache. It never bundles or silently updates dictionary
  data.
- `ferrolex-node` is the selected direct non-Rust adapter. It should expose the
  same engine concepts rather than grow a separate spell-checker.

The root `ferrolex` crate is the one-product Rust release boundary. It
re-exports the supported product crates as `ferrolex::hunspell`,
`ferrolex::suggest`, and `ferrolex::dictionaries`, with common entry points
also available at the crate root. The remaining workspace crates are not part
of that public package boundary.

## Supporting and historical crates

- `ferrolex-cli` is a reference and diagnostic interface for the engine and
  managed dictionary workflow.
- `ferrolex-compiler` and compiled artifacts are implementation and deployment
  tools. They are not a separate product promise without a demonstrated
  consumer need.
- `ferrolex-text` and `ferrolex-code` are existing generic helpers. They do not
  own Markdown, PO, TypeScript, or other format semantics and must not acquire
  parser dependencies.
- The C ABI, Python, LSP, and VS Code packages are prototypes outside the
  current product and distribution scope.

## Consumer-owned format integration

Document-aware projects own parsing and selection of human-language content.
Ferromark can pass Markdown prose, Ferrocat can pass translatable PO strings,
and OXC can pass selected TypeScript comments, strings, or identifiers to the
same ferrolex dictionary and suggestion APIs. Ferrolex does not embed their
parsers or duplicate their syntax policies.

## Normalization boundary

`Normalization::Exact` preserves the supplied UTF-8 form. `Nfc` and `Nfkc`
transform both dictionary entries and lookup queries before comparison. None of
these options case-fold: casing is language-sensitive and will receive its own
contract rather than being silently coupled to Unicode normalization.
