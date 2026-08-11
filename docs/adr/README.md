# Architecture Decision Records

This directory holds the project's durable decisions and their rationale.

## Convention

- **Lifecycle: living records.** Each file always contains the *current*
  decision and is edited in place when the decision evolves. Earlier versions
  live in Git history; there is no supersession chain. This trades a weaker
  in-document audit trail for cheaper reads — one file per decision is always
  authoritative.
- **Naming:** zero-padded number plus kebab-case slug, e.g.
  `0001-code-provenance-policy.md`. Numbers give stable ordering; the file
  identity never changes even as content evolves.
- **Status vocabulary:** `proposed`, `accepted`, `rejected`, `deprecated`.
  (`superseded` is unused — living records are updated, not replaced.)
- **Change tracking:** every record carries a `Last updated` date. Update it
  on every semantic edit.
- **Language:** US English (see ADR-0003).

ADRs hold rationale and direction. Exact values and enforcement live in code,
configuration, CI, and the RFC ([rfc.md](../../rfc.md)).

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-code-provenance-policy.md) | Pragmatic code provenance policy | accepted |
| [0002](0002-native-only-performance-focus.md) | Native-only, CPU- and memory-optimized engine | accepted |
| [0003](0003-project-language-us-english.md) | Project language is US English | accepted |
| [0004](0004-conventional-commits-and-release-please.md) | Conventional Commits and Release Please | accepted |
| [0005](0005-project-name-ferrolex.md) | Project, crates, and CLI binary are named ferrolex | accepted |
| [0006](0006-compiled-format-safety-and-layout.md) | Compiled format: owned loading, little-endian, byte-identical | accepted |
| [0007](0007-dictionary-distribution.md) | Dictionary distribution via companion repository | accepted |
| [0008](0008-no-cspell-compatibility.md) | Own directive format, no cspell compatibility | accepted |
