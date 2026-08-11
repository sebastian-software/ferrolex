# Neutral linguistic dictionary IR

`ferrolex-compiler` owns `DictionaryIr`, the source-neutral, owned
representation between dictionary import and compiled-artifact generation.
It contains declared recognition and ranking semantics only. Runtime indexes,
candidate caches, and pre-expanded word forms are intentionally excluded.

The current Hunspell importer lowers every supported semantic field into this
IR and exposes it through `ImportResult::ir()`. Exact word lists use the same
model through `ExactDictionaryIr::as_dictionary_ir()`: they populate lexemes
and leave the richer fields at their documented defaults.

## Coverage

`DictionaryIr` retains lexemes, flag mode and values, morphology references,
prefix/suffix rules with conditions and continuation flags, casing behavior,
special flags, compound configuration, break patterns, tokenizer metadata,
replacement rules, character removal, conversions, and `FULLSTRIP`/
`COMPLEXPREFIXES`. This is the input contract for a future standalone
artifact; it must not silently project a rich dictionary to a word list.

## hu_HU and ar expressiveness spike

The opt-in real-world fixture suite imports and cache-roundtrips the pinned
Hungarian and Arabic dictionaries. They exercise the edge cases that prevented
freezing an exact-word-only IR:

| Fixture | Observed supported constructs | IR fields |
| --- | --- | --- |
| `hu_HU` | `AF`/`AM`, continuation affixes, `IGNORE`, `ICONV`, compounds, `BREAK`, casing | lexeme/rule flags and morphology; conversions; compound and break configuration |
| `ar` | numeric flags, `AF`/`AM`, `IGNORE`, `ICONV`, Arabic prefix/suffix rules | flag mode; lexeme/rule morphology; conversions; affix rules |

Unsupported source directives remain diagnostics and are not represented as
guessed IR semantics. The full fixture suite is opt-in with
`FERROLEX_COMPAT_FIXTURES`; it verifies the pinned source bytes, recognition
probes, and runtime-cache roundtrip.

## Ownership boundary

The IR owns its strings and collections. An importer may discard its source
text after producing it, and an artifact compiler can retain or serialize it
without tying the result to a source-specific runtime object. Consumers should
treat it as a semantic model, not a stable wire format; artifact compatibility
is owned by the compiled-format version and feature gates.
