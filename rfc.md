# Requirements Specification: Modern Rust Spell-Checking Engine

## 1. Project Summary

The project shall provide a modern, high-performance spell-checking engine written in Rust.

The engine shall be designed as an independent implementation built from first principles and informed by the established concepts, public behavior, documented file formats, and practical lessons of existing spell-checking systems such as Hunspell, Nuspell, and CSpell.

The implementation must not be a port, translation, derivative implementation, or code-level reimplementation of Hunspell, Nuspell, CSpell, or any other existing spell-checking engine.

The core project shall be suitable for licensing under:

```text
MIT OR Apache-2.0
```

The project shall combine two capabilities that are traditionally handled by separate categories of tools:

1. Linguistically capable spell checking using existing Hunspell-compatible dictionaries and morphology rules.
2. Developer-oriented spell checking for source code, identifiers, configuration files, documentation, and other structured technical content.

The project should therefore support the existing Hunspell dictionary ecosystem while using a modern internal architecture that is independent of Hunspell's implementation and internal data structures.

---

# 2. Vision

The project should become a general-purpose spelling infrastructure library rather than merely another Hunspell implementation.

Its conceptual architecture should be:

```text
Input formats
     │
     ├── Hunspell .aff/.dic
     ├── Plain word lists
     ├── Project dictionaries
     ├── Custom dictionary formats
     └── Future import formats
             │
             ▼
      Dictionary importers
             │
             ▼
      Neutral internal model
             │
             ▼
      Dictionary compiler
             │
             ▼
   Optimized native dictionary
             │
             ▼
       Spell-checking core
             │
      ┌──────┼────────┐
      ▼      ▼        ▼
   Lookup  Suggest  Analyze
             │
             ▼
       Higher-level analyzers
             │
      ┌──────┼─────────┐
      ▼      ▼         ▼
    Text   Source    Markup
```

Hunspell compatibility shall therefore be treated primarily as an input and interoperability feature rather than as the architectural foundation of the engine.

---

# 3. Primary Goals

The project shall aim to provide the following characteristics.

## 3.1 Independent implementation

All production code shall be independently written.

No source code shall be copied, translated file-by-file, mechanically converted, or reproduced through side-by-side porting from:

- Hunspell
- Nuspell
- Spellbook
- CSpell
- Aspell
- MySpell
- or other implementations whose provenance is incompatible with MIT OR Apache-2.0

Studying existing implementations — including reading their source code — is permitted and encouraged for:

- publicly documented behavior
- publicly documented formats
- observable runtime behavior
- conceptual and algorithmic understanding
- interoperability expectations
- feature discovery
- performance comparison

The boundary is expression, not knowledge: ideas, algorithms, file formats, and observed behavior are free to reimplement. The concrete code of incompatible implementations must not serve as a template for new code.

Spellbook deserves an explicit rule: it is MPL-2.0 licensed and a self-described Rust rewrite of Nuspell, which makes code proximity to it both the most tempting and the most easily detected. Spellbook must not be used as porting material under any circumstances.

AI-assisted contributions shall be treated as contributions of unknown provenance: generated code must be reviewed for obvious structural closeness to known implementations before merging, and prompts should be phrased against project-owned behavior documentation (§5.3) rather than requesting reproductions of specific implementations.

---

## 3.2 Permissive licensing

The engine itself shall be distributable under:

```text
MIT OR Apache-2.0
```

Users must be able to:

- embed the engine in commercial products
- statically link it
- dynamically link it
- redistribute binaries
- redistribute modified versions
- use it in proprietary software
- use it in SaaS systems
- use it in desktop applications
- use it in developer tools

without copyleft obligations originating from the engine itself.

Dictionary licenses are explicitly outside the licensing scope of the core engine and must be handled separately.

---

## 3.3 Hunspell dictionary ecosystem compatibility

The engine shall support loading and processing commonly used Hunspell-compatible:

```text
.aff
.dic
```

files.

The goal is broad practical compatibility with the existing Hunspell dictionary ecosystem.

Compatibility does not require bug-for-bug reproduction of Hunspell.

The implementation should support the semantics required by real-world dictionaries while remaining free to implement those semantics using completely different algorithms and internal representations.

---

## 3.4 Excellent source-code spell checking

The engine shall treat source code and structured technical text as first-class use cases.

It shall support concepts such as:

- camelCase
- PascalCase
- snake_case
- SCREAMING_SNAKE_CASE
- kebab-case
- dot.separated.names
- identifiers containing digits
- acronyms
- initialisms
- URLs
- email addresses
- file paths
- package names
- domain names
- programming-language keywords (as configurable vocabulary or ignore layers)
- generated identifiers
- mixed identifiers and natural-language text

Tokenization and identifier segmentation shall be independent of the dictionary implementation.

---

## 3.5 High performance

Performance is a defining requirement, not an optimization pass: the engine shall be consistently CPU-optimized for native execution, with memory-saving operations wherever feasible (see §31).

The engine shall be optimized for:

- low lookup latency
- high throughput
- efficient memory usage
- fast process startup
- efficient dictionary loading
- parallel workloads
- large source repositories
- long-running language servers
- CI environments

The architecture should allow precompiled dictionaries to eliminate repeated parsing and preprocessing of textual dictionary files at runtime.

---

# 4. Non-Goals

The following are explicitly not initial project goals.

## 4.1 Exact Hunspell implementation compatibility

The project shall not attempt to reproduce:

- Hunspell's internal algorithms
- Hunspell's class structure
- Hunspell's memory layout
- Hunspell's implementation quirks
- undocumented implementation-specific behavior
- historical bugs
- identical suggestion ordering

Compatibility shall be defined in terms of documented and observable behavior, not implementation identity.

---

## 4.2 Identical suggestion output

The suggestion engine does not need to return exactly the same suggestions as Hunspell or Nuspell.

Suggestion quality is more important than identical output.

---

## 4.3 Bundling dictionaries with the core library

The core engine should initially ship without language dictionaries.

Language dictionaries should be distributed separately so that:

- the core remains license-clean
- individual dictionary licenses remain explicit
- applications can select dictionaries appropriate to their licensing requirements

ferrolex does not redistribute language dictionaries. Its optional dictionary
installer fetches catalogued upstream source pairs only on explicit request,
verifies their immutable revision and SHA-256 digests, records per-locale
license-notice evidence, and writes them to a caller-selected local cache
(ADR-0007). Normal checking, compilation, tests, and CI remain offline;
any downstream distribution of compiled artifacts retains the applicable
dictionary-license obligations.

---

## 4.4 Natural-language grammar checking

Grammar checking, style checking, sentence rewriting, and large-language-model functionality are outside the initial scope.

The initial project shall focus on:

- word recognition
- morphology
- compounds
- spelling suggestions
- tokenization
- identifier analysis

---

## 4.5 Browser and WebAssembly targets

Browser deployment and WebAssembly are not goals of this project, initial or otherwise.

The engine targets native execution. Architecture, dependency, and data-structure decisions shall be made in favor of native CPU performance; wasm32 compatibility shall not act as a constraint (see §31).

---

## 4.6 Brand-term validation tooling

Brand-term validation (checking the correct usage of product and brand names such as Lexios in code, documentation, and localization content — previously explored as `ferrolex-brand` in the earlier ferrolex workspace) is out of scope for this project and shall live in a separate project.

The engine's public capabilities — custom dictionaries, dictionary layering, and forbidden words — should make such tooling straightforward to build as a downstream consumer.

---

# 5. Licensing and Code Provenance Requirements

Code provenance is a fundamental project requirement.

Clean provenance is a product feature, not merely a legal safeguard. The credibility of the MIT OR Apache-2.0 claim is one of this project's core differentiators; its audience is commercial adopters performing license due diligence and an open-source community judging the project's independence. Provenance documentation shall therefore be maintained with the same care as user-facing features.

## 5.1 Original implementation

Every contribution must be either:

1. original work created for this project, or
2. derived from code whose license is explicitly compatible with the project's MIT OR Apache-2.0 licensing model.

Contributors must not submit code translated or ported from incompatible implementations. Knowledge gained by studying such implementations may inform independently written code (§3.1).

---

## 5.2 Clean implementation policy

The repository shall include a documented policy similar to:

```text
This project is an independent implementation.

Studying existing spell checkers — including their source code — is
welcome. Copying, file-by-file translation, mechanical conversion, or
side-by-side porting of code from Hunspell, Nuspell, Spellbook, or other
incompatible implementations is not.

Spellbook is explicitly off-limits as porting material: as a Rust
implementation derived from Nuspell, code proximity to it is both the most
likely temptation and the most easily detected.

Compatibility work should be grounded in publicly documented formats,
project-owned behavior documentation, independently created test cases,
and observable behavior of existing implementations.

AI-generated code is treated as a contribution of unknown provenance and
must be reviewed for obvious closeness to known implementations before
merging.

All contributed production code must have clear provenance compatible with
this project's licensing model.
```

This policy should be included in `CONTRIBUTING.md`.

---

## 5.3 Independent compatibility documentation

Hunspell behavior needed by the project should be described in project-owned documentation.

For example:

```text
docs/
  hunspell-format.md
  affix-semantics.md
  compound-semantics.md
  compatibility.md
```

These documents must be written independently and in the project's own words.

They should describe behavior rather than implementation.

For the most intricate areas — affix and compound semantics in particular — these documents serve as the primary reference that implementation and tests are written against.

---

## 5.4 Test provenance

Test cases should preferably be independently created.

Synthetic dictionaries should be used whenever practical.

For example:

```text
tests/fixtures/
  prefix-basic/
  suffix-basic/
  cross-product/
  forbidden-word/
  circumfix/
  compound-basic/
  compound-complex/
  capitalization/
```

Existing test suites from incompatible projects must not simply be copied into the repository.

---

# 6. Architecture

The architecture should separate dictionary formats, linguistic behavior, storage, suggestions, and source-code analysis.

A possible workspace structure is:

```text
crates/

  ferrolex-core/
  ferrolex-morphology/
  ferrolex-hunspell/
  ferrolex-compiler/
  ferrolex-suggest/
  ferrolex-code/
  ferrolex-cli/
```

Exact crate names may change, but separation of concerns should remain.

---

# 7. Core Engine

The core engine shall expose a small, stable API.

Conceptually:

```rust
pub trait Dictionary {
    fn contains(&self, word: &str) -> bool;
}
```

Higher-level functionality may expose interfaces conceptually similar to:

```rust
pub trait Speller {
    fn check(&self, word: &str) -> bool;
}

pub trait Suggester {
    fn suggest(&self, word: &str) -> Vec<Suggestion>;
}

pub trait Morphology {
    fn analyze(&self, word: &str) -> Vec<Analysis>;
}
```

The actual API should prioritize:

- zero-copy operations where practical
- predictable allocations
- ergonomic Rust usage
- thread safety
- immutable shared dictionaries
- compatibility with async applications without requiring async internally

Suggestion APIs should offer allocation-conscious variants — writing into a caller-provided buffer or yielding candidates through an iterator — in addition to convenience methods returning `Vec`.

---

# 8. Dictionary Import Architecture

Dictionary parsing shall be separated from runtime lookup.

A dictionary importer shall transform an external dictionary format into a neutral intermediate representation.

Conceptually:

```text
Hunspell .aff + .dic
         │
         ▼
    Hunspell parser
         │
         ▼
      Neutral IR
         │
         ▼
 Dictionary compiler
```

Future formats should be able to use the same pipeline:

```text
word list ─────────────┐
                       │
Hunspell ──────────────┼──► IR ──► compiler
                       │
custom format ─────────┘
```

The spell-checking core must not depend conceptually on Hunspell's file format.

---

# 9. Neutral Intermediate Representation

The internal representation should describe linguistic behavior rather than Hunspell syntax.

Possible concepts include:

```text
Dictionary
├── Lexemes
│   ├── normalized form
│   ├── flags / capabilities
│   ├── morphological metadata
│   └── restrictions
│
├── Prefix Rules
├── Suffix Rules
├── Compound Rules
├── Case Rules
├── Replacement Rules
├── Character Rules
└── Suggestion Metadata
```

The exact representation should be designed around efficient compilation and lookup rather than around preserving the textual `.aff` representation.

The IR is internal and unstable until 1.0. Its expressiveness shall be validated early with a spike against the hardest constructs from real dictionaries (`hu_HU`, `ar`) before the design freezes (see Phase 2).

---

# 10. Hunspell Importer

The Hunspell importer shall parse `.aff` and `.dic` files and translate supported constructs into the project's internal representation.

The importer should progressively support commonly encountered Hunspell functionality, including where applicable:

- dictionary entries
- flags
- flag encoding modes
- prefix rules
- suffix rules
- cross-product affixes
- continuation classes
- conditional affixes
- circumfixes
- forbidden words
- keep-case semantics
- need-affix behavior
- pseudoroots
- homonyms
- compound rules
- compound minimum lengths
- compound flags
- replacement rules (`REP`)
- character conversion rules (`ICONV`, `OCONV`)
- capitalization behavior
- Unicode
- language-specific casing
- legacy character encodings declared via `SET` (e.g., ISO-8859-1, ISO-8859-2, KOI8-R), decoded to UTF-8 during import
- flag aliases (`AF`, `AM`)
- suggestion-related directives (`TRY`, `KEY`, `MAP`, `PHONE`, `MAXDIFF`, `MAXNGRAMSUGS`)
- `NOSUGGEST` (words that are recognized but must never be offered as suggestions, e.g., profanity)
- `WARN` and `FORBIDWARN`
- `BREAK` word-breaking rules
- `WORDCHARS` tokenization hints
- `IGNORE` characters (e.g., optional diacritics)
- `COMPLEXPREFIXES` (two-step prefix stripping, required for Arabic and Hebrew)
- complex morphology required by widely used dictionaries

Feature implementation shall be driven by actual dictionary ecosystem requirements rather than the goal of reproducing every historical directive immediately.

---

# 11. Compatibility Levels

Compatibility shall be explicitly measurable.

A useful classification is:

## Level 1: Format compatibility

The engine can parse a given Hunspell dictionary.

## Level 2: Recognition compatibility

For the supported feature subset, accepted and rejected words should normally match expected Hunspell behavior.

## Level 3: Morphological compatibility

Complex affix and compound behavior is correctly interpreted.

## Level 4: Ecosystem compatibility

Major real-world Hunspell dictionaries operate correctly without modification.

The project does not promise identical suggestion ordering or undocumented bug compatibility.

Compatibility shall be measured, not asserted:

- A fixed reference set of dictionaries anchors testing, for example: `en_US`, `de_DE`, `fr_FR`, `nl_NL`, `hu_HU`, `ar`, `tr_TR`.
- Hungarian (`hu_HU`) is the recognized stress test for affix machinery and shall be exercised early — during Phases 2–3 — rather than deferred to Level 4 validation.
- Recognition agreement with Hunspell (accept/reject decisions over a word corpus) shall be tracked per dictionary as a scorecard for the supported feature set.

---

# 12. Dictionary Compiler

A major architectural goal shall be the ability to compile source dictionaries into an optimized binary format.

For example:

```bash
ferrolex compile de_DE.aff de_DE.dic -o de_DE.spell
```

The resulting format should:

- load substantially faster than parsing `.aff` and `.dic`
- avoid unnecessary runtime allocations
- support direct or near-direct lookup
- support memory mapping where practical
- include a format version
- include feature metadata
- include source dictionary metadata
- allow compatibility checks
- be deterministic
- be reproducible

---

# 13. Memory-Mapped Dictionaries

The architecture should investigate memory-mapped dictionaries as a primary optimization strategy.

The desired runtime model is:

```text
traditional:

start process
    ↓
read .aff
    ↓
parse .aff
    ↓
read .dic
    ↓
parse .dic
    ↓
allocate structures
    ↓
build indexes
    ↓
ready


preferred:

start process
    ↓
mmap compiled dictionary
    ↓
ready
```

The compiled format should therefore avoid unnecessary pointer-heavy structures and favor representations suitable for serialization and memory mapping.

Memory mapping interacts with the security requirements (§39): a mapped file can be modified by other processes while in use, and its bytes must be treated as untrusted at all times. The format is therefore designed to tolerate arbitrary bytes without undefined behavior (ADR-0006): every access is bounds-checked, and corrupted input may at worst produce wrong results, never memory unsafety. Loading performs only a fast header and checksum check; full structural validation is available as an opt-in (`ferrolex validate`, CI, paranoid mode).

The compiled format is little-endian, uses 8-byte-aligned sections addressed by offsets rather than pointers, and is byte-identical across platforms for the same input, compiler version, and options.

---

# 14. Lookup Data Structures

The implementation should evaluate appropriate modern data structures rather than assuming the traditional Hunspell approach.

Candidates may include:

- compact tries
- radix tries
- finite-state transducers
- minimal deterministic automata
- perfect hashing
- compact hash tables
- sorted tables with binary search
- hybrid representations

Different structures may be appropriate for:

- exact words
- stems
- prefixes
- suffixes
- compound rules
- suggestion candidate generation

Performance shall be benchmark-driven.

---

# 15. Morphology Engine

Morphology shall be implemented independently from dictionary parsing.

The morphology layer should understand abstract concepts such as:

```text
stem
  +
prefix transformation
  +
suffix transformation
  +
conditions
  +
continuation rules
  =
accepted word form
```

It should support languages with significant morphology without requiring expansion of every possible generated word into memory.

The engine must avoid assuming that all languages behave like English.

Special attention should eventually be given to languages commonly used to test spell-checker sophistication, including:

- German
- Hungarian
- Turkish
- Dutch
- Arabic (complex prefix morphology)
- languages requiring non-trivial Unicode casing

Finnish is deliberately not listed: its morphology exceeds what the Hunspell dictionary ecosystem realistically provides and is traditionally served by dedicated tools such as Voikko.

---

# 16. Compound Words

Compound handling shall be treated as a first-class capability.

This is particularly important for German and Dutch.

The engine should support:

- simple compounds
- dictionary-controlled compound eligibility
- minimum component length
- compound position restrictions
- compound pattern rules
- morphology within compound components
- invalid compound combinations
- case-sensitive compound behavior

The design should avoid naïve exponential segmentation whenever possible.

Suitable pruning and indexing strategies should be investigated.

---

# 17. Unicode

Unicode correctness is a mandatory requirement.

The engine must not assume ASCII.

The implementation shall correctly address:

- UTF-8 input
- Unicode scalar boundaries
- case conversion
- normalization considerations
- combining characters
- language-specific case behavior
- German `ß`
- Turkish dotted and dotless `i`
- characters outside the Basic Multilingual Plane where relevant

The runtime core is strictly UTF-8. Legacy dictionary encodings from the Hunspell ecosystem are handled at the import boundary (§10): the importer decodes them to UTF-8, and nothing downstream of the importer deals with non-UTF-8 data.

Rust's standard Unicode functionality should be preferred where sufficient.

External Unicode dependencies should only be introduced when justified by missing functionality.

A mandatory dependency on ICU should preferably be avoided unless there is a strong technical reason.

---

# 18. Normalization

Normalization must be explicit and configurable.

The engine should distinguish between:

- original token
- normalized lookup representation
- case-folded representation
- suggestion display representation

The engine must avoid silently destroying information required for correct case-sensitive behavior.

---

# 19. Suggestion Engine

Suggestion generation shall be architecturally independent from word recognition.

Conceptually:

```text
misspelled word
      │
      ▼
candidate generation
      │
      ├── edit operations
      ├── dictionary replacement rules
      ├── n-gram similarity
      ├── phonetic similarity
      ├── keyboard proximity
      ├── morphology
      ├── compound correction
      └── casing corrections
              │
              ▼
            ranking
              │
              ▼
          suggestions
```

Candidate generation and candidate ranking should be separate components.

---

# 20. Suggestion Ranking

Suggestion ranking should be designed independently rather than attempting to reproduce Hunspell ordering.

Possible ranking signals include:

- weighted edit distance
- n-gram similarity
- character transposition
- keyboard adjacency
- dictionary replacement rules
- language-specific replacement patterns
- prefix similarity
- suffix similarity
- morphology
- word frequency
- capitalization
- compound structure
- phonetic similarity

The API should allow future ranking improvements without changing dictionary semantics.

---

# 21. Frequency Information

The architecture should allow dictionaries to optionally contain frequency information.

Frequency information may be used to rank suggestions.

It must not be required for basic word recognition.

This allows future dictionaries to distinguish, for example:

```text
common word
rare word
technical term
archaic word
```

without changing the fundamental spell-checking model.

Frequency data is derived from corpora that carry their own licenses; the provenance requirements of §5 apply to frequency sources as well.

---

# 22. Dictionary Layers

Multiple dictionaries shall be composable.

For developer use cases, an effective dictionary may consist of:

```text
en-US
  +
software terminology
  +
programming language vocabulary
  +
framework vocabulary
  +
organization vocabulary
  +
project vocabulary
  +
user vocabulary
```

The API should support efficient dictionary layering without requiring all dictionaries to be physically merged.

One layer type shall be a mutable user overlay: "add to dictionary" must take effect immediately at runtime without recompiling or reloading base dictionaries. Base dictionaries remain immutable (§30); mutability is confined to small overlay layers with their own thread-safe update and persistence story.

Conceptually:

```rust
let checker = Checker::builder()
    .dictionary(english)
    .dictionary(software)
    .dictionary(typescript)
    .dictionary(project)
    .build();
```

---

# 23. Source-Code Analyzer

Source-code spell checking shall be implemented above the core spell-checking engine.

The source analyzer shall be responsible for extracting potentially meaningful words from structured input.

The dictionary engine itself should not need to know whether a token originated from Rust, JavaScript, Markdown, or plain text.

---

# 24. Identifier Segmentation

The analyzer shall support segmentation such as:

```text
userAuthenticator
→ user
→ authenticator

OAuthAuthenticationProvider
→ OAuth
→ Authentication
→ Provider

HTTPResponseCode
→ HTTP
→ Response
→ Code

user_profile_image
→ user
→ profile
→ image
```

Segmentation should correctly handle boundaries involving:

- lowercase to uppercase
- acronym to normal word
- underscores
- hyphens
- digits
- symbols
- Unicode letters

Exact behavior should be configurable.

Segmentation must also work in reverse: suggestions for a misspelled segment shall be recombined case-preservingly into a complete identifier (for example `OAuthAuthentcationProvider` → `OAuthAuthenticationProvider`), so that tools can offer whole-identifier replacements.

---

# 25. Token Classification

The analyzer should classify tokens where useful.

Possible token classes include:

```text
NaturalWord
Identifier
Acronym
URL
Email
Path
Domain
Number
Hash
GeneratedToken
Unknown
```

Different checking policies may then apply to each category.

---

# 26. Source Language Integration

The first version does not need full parsers for every programming language.

The architecture should support progressively more sophisticated integrations.

Possible levels include:

```text
Level 1:
generic tokenizer

Level 2:
file-type-aware tokenization

Level 3:
tree-sitter or parser integration

Level 4:
language-specific semantic analysis
```

The core package should not require language-specific parsers.

---

# 27. Ignore Mechanisms

Developer-oriented spell checking requires robust ignore behavior.

The analyzer should support ignoring:

- URLs
- UUIDs
- hashes
- hexadecimal values
- generated identifiers
- base64-like content
- binary-looking data
- dependency names
- selected file patterns
- selected token categories
- configured regular expressions

The analyzer shall additionally support inline comment directives in checked files, for example:

```text
ferrolex:ignore <words>
ferrolex:disable / ferrolex:enable
```

The directive format is ferrolex's own. Compatibility with other checkers' directive families or configuration formats (e.g., cspell) is explicitly not promised (ADR-0008).

Users should also be able to add words to:

- global dictionaries
- workspace dictionaries
- file-local ignore lists

---

# 28. Public API Layers

The project should expose multiple abstraction levels.

## Low-level dictionary API

```rust
dictionary.contains("authentication")
```

## Suggestion API

```rust
dictionary.suggest("authentcation")
```

## Text API

```rust
checker.check_text("This contains a misspeled word.")
```

## Identifier API

```rust
checker.check_identifier("OAuthAuthentcationProvider")
```

## Document analyzer API

```rust
analyzer.check(source, Language::Rust)
```

Consumers should not be forced to use the source-code functionality when they only require a dictionary engine.

---

# 29. Rust API Requirements

The Rust API should be:

- idiomatic
- strongly typed
- safe
- thread-safe
- allocation-conscious
- easy to embed
- stable enough for downstream tooling

Unsafe Rust may be used for carefully justified performance optimizations such as memory mapping, but:

- unsafe usage should be minimized
- unsafe invariants must be documented
- safe public APIs must encapsulate unsafe internals

---

# 30. Concurrency

Loaded dictionaries should preferably be immutable and cheaply shareable across threads.

Typical usage should support:

```rust
Arc<Dictionary>
```

without requiring global locks for ordinary lookup operations.

Spell-check operations should be parallelizable.

Immutability applies to loaded base dictionaries; mutable user overlays (§22) are the explicit exception and must provide their own thread-safe update mechanism.

---

# 31. CPU and Memory Efficiency

The engine shall be consistently optimized for native CPU execution, with memory-saving operations wherever feasible.

The implementation should favor:

- cache-friendly, contiguous data layouts over pointer-heavy structures
- allocation-free hot paths for ordinary lookup
- amortized or arena allocation where allocation is unavoidable
- compact encodings (interned strings, small-integer flag sets) when they reduce memory footprint and cache pressure
- SIMD only where benchmarks demonstrate a measurable win
- data structures sized for realistic dictionary workloads rather than theoretical worst cases

When memory savings and hot-path latency conflict, latency wins and the trade-off shall be documented.

Portability to non-native targets shall not constrain these optimizations (§4.5).

---

# 32. Native Integration

The architecture should allow future bindings for languages and environments other than Rust.

Potential interfaces include:

- C ABI
- Node.js
- Python
- Swift
- Kotlin
- Java
- .NET

A stable C-compatible API may eventually serve as the lowest common denominator.

These integrations are not required for the initial milestone.

---

# 33. Command-Line Interface

A CLI shall be provided for development, validation, and general use.

Potential commands:

```text
ferrolex check
ferrolex suggest
ferrolex analyze
ferrolex compile
ferrolex inspect
ferrolex benchmark
ferrolex validate
```

Examples:

```bash
ferrolex check README.md

ferrolex suggest authentcation

ferrolex compile de_DE.aff de_DE.dic -o de_DE.spell

ferrolex validate de_DE.aff de_DE.dic

ferrolex inspect de_DE.spell
```

The CLI binary and the project share the name `ferrolex` (ADR-0005); this avoids the name collision with the classic Unix `spell` tool.

---

# 34. Dictionary Validation

The tooling should provide useful diagnostics for malformed or unsupported dictionaries.

Diagnostics should contain:

- file
- line
- directive
- severity
- explanation

For example:

```text
de_DE.aff:1842

Unsupported directive: XYZ

warning: this directive is currently ignored and may affect
compound-word recognition.
```

Unsupported features should not silently produce incorrect results where detection is possible.

Loading behavior for unsupported or malformed constructs shall be configurable:

- strict: fail loading (suitable for CI)
- lenient: load with diagnostics and degrade predictably (default)

---

# 35. Feature Reporting

Compiled dictionaries should record which features they require.

The runtime and CLI should be able to report something similar to:

```text
Dictionary: de_DE
Format: Hunspell
Compiler version: 0.4.0

Features:
✓ prefixes
✓ suffixes
✓ cross-product
✓ compounds
✓ forbidden words
✓ replacement rules
✗ phonetic rules
```

This makes partial compatibility measurable and transparent.

---

# 36. Compatibility Testing

Compatibility shall be tested using multiple strategies.

## 36.1 Synthetic semantic tests

Small independently created dictionaries shall test individual features in isolation.

Example:

```text
stem: party

suffix rule:
  y → ies

expected accepted:
  party
  parties

expected rejected:
  partys
```

Synthetic fixtures should minimize ambiguity.

---

## 36.2 Real-world dictionary tests

Widely used dictionaries should be loaded and exercised.

The purpose is ecosystem interoperability.

Dictionary licenses must be respected and fixtures must only be committed when redistribution is permitted.

Dictionaries that cannot be redistributed shall be downloaded at test time (and cached) rather than committed to the repository.

---

## 36.3 Black-box differential testing

Existing spell checkers may be executed externally during development as compatibility oracles.

For example:

```text
test dictionary
      │
      ├── Hunspell → result A
      ├── Nuspell  → result B
      └── project  → result C
```

Differences can then be investigated.

Production code must not depend on these implementations.

Compatibility tools using them should remain optional development tooling.

---

# 37. Fuzz Testing

Parsers and runtime structures shall be fuzz-tested.

Priority fuzz targets include:

- `.aff` parser
- `.dic` parser
- compiled dictionary loader
- Unicode input
- malformed flags
- compound evaluation
- suggestion generation

The engine must not crash or exhibit undefined behavior when processing malformed dictionary data.

---

# 38. Property-Based Testing

Property-based tests should be used where appropriate.

Examples:

- serialization followed by deserialization preserves semantics
- dictionary compilation is deterministic
- checking a valid generated affix form succeeds
- normalization is idempotent
- lookup does not mutate shared dictionary state
- suggestion output contains no invalid UTF-8

---

# 39. Security

Dictionaries must be treated as untrusted input.

The parser shall be hardened against:

- integer overflow
- pathological allocation
- excessive recursion
- maliciously large rule counts
- malformed UTF-8 where applicable
- denial-of-service through pathological compound rules
- denial-of-service through pathological suggestion inputs (e.g., extremely long words)
- decompression-like expansion behavior
- invalid compiled dictionary files

Explicit limits should exist where necessary.

---

# 40. Performance Benchmarks

Performance shall be continuously measured.

Benchmarks should include:

## Startup

```text
parse textual dictionary
load compiled dictionary
memory-map compiled dictionary
```

## Lookup

```text
valid common word
invalid word
affixed word
compound word
mixed-case word
```

## Suggestions

```text
single edit
transposition
long word
compound typo
no useful suggestion
```

## Developer workloads

```text
large Markdown repository
TypeScript repository
Rust repository
mixed documentation/code repository
```

---

# 41. Performance Targets

Initial targets should focus on relative rather than arbitrary absolute numbers.

The engine should aim to:

- outperform traditional Hunspell implementations for high-volume lookup
- provide substantially faster startup when using compiled dictionaries
- keep ordinary exact-word lookup allocation-free
- keep dictionary memory usage predictable
- scale well across CPU cores
- avoid runtime reconstruction of indexes when a compiled dictionary is available

Performance claims must be supported by reproducible benchmarks.

---

# 42. Determinism

Dictionary compilation shall be deterministic.

Given:

- the same dictionary files
- the same compiler version
- the same relevant options

the compiler shall generate byte-identical output on every platform.

This facilitates:

- caching
- reproducible builds
- content-addressable storage
- CI optimization
- package distribution

Runtime behavior shall be deterministic as well: given the same dictionary and input, `check()` and `suggest()` return identical results across runs and platforms. Suggestion effort shall be bounded by deterministic work budgets (candidate counts, edit operations) rather than wall-clock time — Hunspell's internal time limits make its suggestion output nondeterministic, which this project explicitly avoids.

---

# 43. Binary Dictionary Versioning

The native compiled format shall contain an explicit format version.

Backward compatibility policy should distinguish between:

```text
source dictionary compatibility
public Rust API compatibility
compiled binary format compatibility
```

The project should not promise perpetual binary-format stability during early development.

---

# 44. Error Handling

Library APIs shall return structured errors.

The engine should avoid:

- panics for malformed user input
- string-only errors when structured information is available
- silently ignoring important unsupported semantics

Errors should retain context where useful.

---

# 45. Observability

Debugging facilities should allow developers to understand why a word was accepted or rejected.

A future diagnostic API might return:

```text
Word: Häuser
Accepted: yes

Reason:
stem: Haus
suffix rule: ...
dictionary entry: ...
```

For compounds:

```text
Word: Haustürschlüssel
Accepted: yes

Components:
Haus
Tür
Schlüssel
```

This is particularly valuable when debugging dictionary behavior.

---

# 46. Stable vs. Diagnostic APIs

The project should distinguish between:

- stable user-facing APIs
- diagnostic/internal analysis APIs

Internal morphological details may evolve more rapidly than:

```text
check()
suggest()
```

This distinction should be reflected in the public API design.

---

# 47. Configuration

Higher-level checking should support configuration for:

- active languages
- dictionaries
- case sensitivity
- identifier splitting
- allowed compound behavior
- ignored words
- ignored patterns
- suggestion count
- minimum token length
- file types
- paths
- project dictionaries

Configuration should be serializable and usable by CLI, LSP, and library consumers.

---

# 48. Language Server

A dedicated LSP implementation is a desirable later-stage deliverable.

The architecture should allow:

```text
editor
   │
   ▼
spell LSP
   │
   ├── dictionary core
   ├── code analyzer
   └── project configuration
```

The LSP should eventually support:

- diagnostics
- quick fixes
- add-to-dictionary actions
- ignore actions
- configuration reload
- incremental document analysis

The LSP is not required for the first core milestone.

---

# 49. Extensibility

The project should be designed so that new dictionary formats and analysis strategies can be added without rewriting the core.

Potential future importers include:

```text
Hunspell
plain word list
word-frequency list
technical vocabulary word lists (e.g., the MIT-licensed cspell-dicts collection, imported as plain data — see ADR-0008)
custom binary lexicon
application-specific dictionary
```

Potential future analyzers include:

```text
plain text
Markdown
HTML
source code
Git commit messages
documentation
localization resources
```

---

# 50. Dependency Policy

The core should prefer a small dependency surface.

Dependencies should be evaluated for:

- license compatibility
- maintenance status
- security
- binary size
- performance
- necessity

Core functionality should not depend on large frameworks.

---

# 51. MSRV

A Minimum Supported Rust Version shall be documented.

The project should avoid unnecessarily requiring the latest compiler unless newer Rust functionality provides meaningful benefits.

The MSRV policy may become stricter after the project reaches a stable release.

---

# 52. Documentation

Documentation shall cover at least:

```text
README.md
ARCHITECTURE.md
CONTRIBUTING.md
LICENSE-MIT
LICENSE-APACHE
SECURITY.md

docs/
  adr/
  hunspell-compatibility.md
  dictionary-format.md
  morphology.md
  suggestions.md
  source-code-analysis.md
  binary-format.md
  performance.md
```

---

# 53. Project Identity

The project should not market itself primarily as a Hunspell replacement.

Preferred positioning:

> A modern, high-performance spell-checking engine for natural language and source code, with native support for the Hunspell dictionary ecosystem.

Possible shorter positioning:

> Modern spell checking infrastructure for text and code.

Or:

> A modern Rust spell-checking engine with Hunspell-compatible dictionaries.

The wording "Hunspell port" should be avoided because it is technically inaccurate and creates unnecessary licensing ambiguity.

The project is named `ferrolex` (ADR-0005).

---

# 54. Development Phases

## Phase 1 — Core Lexicon

Implement:

- UTF-8 word handling
- normalization
- exact dictionary lookup
- plain word-list dictionaries
- core Rust API
- benchmarks
- CLI basics

Success criterion:

A fast, stable dictionary lookup engine exists independently of Hunspell.

---

## Phase 2 — Hunspell Parsing

Implement:

- `.dic` parsing
- `.aff` parsing infrastructure
- flag handling
- basic prefixes
- basic suffixes
- conditions
- cross-product behavior
- IR expressiveness spike against the hardest constructs from `hu_HU` and `ar`

Success criterion:

Simple real-world Hunspell dictionaries can be loaded.

---

## Phase 3 — Advanced Morphology

Implement:

- continuation classes
- circumfixes
- forbidden words
- need-affix semantics
- capitalization semantics
- advanced affix behavior

Success criterion:

Broad word-recognition compatibility with common dictionaries.

---

## Phase 4 — Compounds

Implement:

- compound flags
- compound rules
- restrictions
- optimized segmentation
- German-focused testing

Success criterion:

High-quality support for dictionaries such as German that depend heavily on compounds.

---

## Phase 5 — Native Dictionary Compiler

Implement:

- neutral IR serialization
- optimized binary format
- deterministic compiler
- fast loader
- memory-mapping investigation

Success criterion:

Compiled dictionaries start significantly faster than textual dictionaries.

---

## Phase 6 — Suggestions

Implement:

- edit-distance candidate generation
- transpositions
- replacement rules
- casing suggestions
- morphology-aware candidates
- ranking

Success criterion:

Useful production-quality suggestions without requiring compatibility with Hunspell's exact ordering.

---

## Phase 7 — Code Spell Checking

Implement:

- generic tokenizer
- camelCase splitting
- PascalCase splitting
- snake_case splitting
- acronym handling
- URL/path detection
- project dictionaries
- technical dictionary layers

Success criterion:

The engine can serve as the foundation of a practical CSpell-class developer tool.

---

## Phase 8 — Integrations

Evaluate and, where justified, implement:

- C ABI
- Node.js native bindings
- Python bindings
- LSP
- editor integrations

---

# 55. Initial Acceptance Criteria

The first meaningful release should meet the following criteria.

## Licensing

- All engine code is MIT OR Apache-2.0 compatible.
- Code provenance policy is documented.
- No incompatible implementation code has been copied or translated.
- Dictionaries are not implicitly relicensed with the engine.

## Core

- UTF-8-safe exact word lookup works.
- Multiple dictionaries can be composed.
- Dictionary lookup is thread-safe.
- Public APIs do not require unsafe usage.

## Hunspell

- `.dic` files can be parsed.
- `.aff` files can be parsed.
- Common prefix and suffix rules work.
- Cross-product rules work.
- Unsupported directives produce diagnostics.
- Compatibility behavior is independently tested.

## Tooling

- CLI can check a word.
- CLI can check a text file.
- CLI can validate a dictionary.
- Benchmarks are reproducible.

## Quality

- Core parser components have fuzz tests.
- CI runs formatting, linting, tests, and license checks.
- Major architecture and licensing decisions are documented.

---

# 56. Long-Term Success Criteria

The project should eventually be capable of serving as:

- a Rust crate
- a native spell-checking library
- a CLI spell checker
- the engine for an LSP
- the engine for editor extensions
- an embeddable commercial spell-checking component
- a backend spell-checking service
- a replacement for legacy native spell-checking dependencies
- a developer-focused source-code spell checker

without requiring downstream applications to accept copyleft licensing obligations originating from the engine.

---

# 57. Design Principles

When implementation choices conflict, use the following priorities.

## 1. Correctness over compatibility with bugs

Correct documented behavior is preferable to reproducing historical implementation quirks.

## 2. Behavior over implementation similarity

Compatibility should concern observable results, not internal architecture.

## 3. Independent design over transliteration

If an existing engine solves a problem in a particular way, understand the problem and design an appropriate Rust solution independently.

## 4. Fast runtime over fast parser implementation

It is acceptable for dictionary compilation to perform significant preprocessing if this produces a much faster runtime representation.

## 5. Explicit behavior over hidden magic

Normalization, tokenization, and compatibility decisions should be inspectable and configurable.

## 6. Composability over monoliths

Dictionary lookup, morphology, suggestions, source analysis, CLI integration, and language-server functionality should remain separable.

## 7. Ecosystem compatibility over historical architecture

Support the enormous value contained in existing Hunspell dictionaries without inheriting Hunspell's implementation constraints.

## 8. Permissive embedding as a first-class requirement

Static linking and commercial embedding must remain straightforward.

## 9. Native CPU performance over portability abstractions

The engine is optimized for native execution: cache-friendly layouts, allocation-free hot paths, and memory-frugal operations take precedence over portability to non-native targets such as WebAssembly.

---

# 58. Conceptual Differentiation

The project's place in the ecosystem can be summarized as follows.

```text
Hunspell
  strong morphology
  large dictionary ecosystem
  legacy architecture

Nuspell
  modernized Hunspell-compatible implementation
  improved performance and Unicode handling
  LGPL licensing

CSpell
  excellent developer-oriented tokenization
  layered technical dictionaries
  source-code focus
  different dictionary architecture

This project
  independent Rust implementation
  permissive MIT/Apache licensing
  Hunspell dictionary compatibility
  modern morphology engine
  compiled native dictionaries
  source-code-aware analysis
  native embedding
```

Beyond the classic engines, several Rust projects already occupy parts of this space:

```text
Spellbook
  Rust rewrite of Nuspell, maintained in the Helix editor ecosystem
  MPL-2.0, inherits Nuspell's design and provenance
  off-limits as source material for this project (§3.1)

zspell
  Rust implementation with Hunspell-format support
  top-level LICENSE is Apache-2.0; per-file notices must be checked
  before using any of it as reference material

typos
  developer-focused typo finder in Rust (MIT OR Apache-2.0)
  intentionally minimal lexicon, no Hunspell dictionary support

Harper
  grammar and spell checking in Rust with an LSP (Apache-2.0)
  own dictionary approach, not focused on the Hunspell ecosystem
```

No existing project combines Hunspell-ecosystem compatibility, permissive licensing, compiled native dictionaries, and source-code-aware checking. That combination is this project's niche.

The objective is not to duplicate any one of these projects.

The objective is to combine lessons learned from decades of spell-checking technology into a new architecture appropriate for modern software.

---

# 59. Core Product Statement

The core product statement should remain:

> Build an independently implemented, permissively licensed, high-performance spell-checking engine in Rust that can consume the existing Hunspell dictionary ecosystem while providing modern APIs and first-class support for source-code spell checking.

The key architectural principle is:

> Hunspell is a supported compatibility format, not the internal architecture.

The key licensing principle is:

> Understand existing behavior and formats, but independently implement every part of the engine under a provenance model suitable for MIT OR Apache-2.0.

The key technical principle is:

> Separate dictionary semantics, runtime representation, suggestion generation, and document tokenization so each can evolve independently.

---

# 60. Project Conventions

- The project language is US English for all repository artifacts: identifiers, comments, documentation, commit messages, decision records, and issues.
- Commit messages follow Conventional Commits.
- Releases are automated with Release Please, using the shared setup from the `sebastian-software/standards` repository.
- Durable decisions are recorded as living Architecture Decision Records under `docs/adr/`. The RFC describes requirements; ADRs carry decision rationale.

---

# 61. Open Questions

The initial open questions have been resolved and are recorded as decision records:

- Naming → ADR-0005
- Memory-mapping strategy and compiled-format details → ADR-0006
- Dictionary distribution → ADR-0007
- cspell interoperability scope → ADR-0008
- Neutral-IR expressiveness → validated via the Phase 2 spike (§9, §54)

New open questions are collected here as they arise.
