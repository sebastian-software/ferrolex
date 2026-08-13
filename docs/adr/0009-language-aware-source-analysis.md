# ADR-0009: Progressive language-aware source analysis

- Status: accepted
- Date: 2026-08-13
- Last updated: 2026-08-13
- Deciders: Sebastian Werner

## Context

RFC §§23–28 require source analysis to stay independent from dictionary
semantics while allowing progressively richer language integration. RFC §49
also names source code, Markdown, and HTML as future analyzers. The completed
source-analysis work (#10) provides a generic analyzer and Level-2
file-type-aware directive presets, but does not make parser boundaries or a
first parser target explicit.

Repository evidence at `origin/main` (`3309245`) favors Rust as the first
source-language target: the workspace contains 26 Rust files and about 21,475
Rust source lines, compared with three JavaScript files and about 256 combined
JavaScript/TypeScript source lines. The maintained LSP is implemented in Rust,
and Rust is one of the synthetic repository-checking benchmark workloads.
TypeScript is also a benchmark workload and backs the small VS Code extension,
but its repository footprint is smaller. Python is an experimental binding
spike, not a primary source-analysis workload. The repository contains no user
request or usage evidence that would justify claiming demand for any one
language.

## Decision

Keep generic analysis as the stable fallback for every language. Define three
support tiers: generic token analysis (Level 1), file-type-aware directive
syntax selection (Level 2), and opt-in parser/semantic integrations (Levels
3–4). A parser-backed implementation must reuse the generic dictionary and
policy contract; semantic behavior is an additional per-language commitment,
not an implication of parsing.

Choose Rust as the first parser-backed analyzer target. Implement only its
syntax-boundary extraction in [#96](https://github.com/sebastian-software/ferrolex/issues/96): comments, string literals, and identifier components with
original UTF-8 ranges and parser-error recovery. Type resolution, macro
expansion, name resolution, compiler integration, new configuration keys, and
other languages remain out of scope.

## Considered options

### Rust parser-backed analysis (chosen)

It aligns with the dominant maintained source workload and existing Rust LSP,
so it adds one parser/grammar maintenance surface where contributors already
work. The cost is keeping that parser compatible with Rust syntax and testing
error recovery, but the follow-up limits the first change to syntax boundaries.

### TypeScript parser-backed analysis (deferred)

It could improve the local VS Code extension path and is present in benchmarks,
but the extension is small and adds a separate language grammar/version
maintenance burden before the primary Rust workload has parser coverage.

### Python parser-backed analysis (deferred)

The experimental binding spike is insufficient workload evidence for a first
maintained parser integration. It would introduce another grammar and support
commitment without a stronger repository-based driver.

## Consequences

- Unsupported languages keep the exact generic analyzer behavior and
  configuration contract documented in `docs/source-code-analysis.md`.
- Rust will gain more accurate syntax boundaries only after #96 is implemented;
  this record does not claim parser or semantic support already exists.
- A future language target needs comparable workload evidence, a maintenance
  assessment, and a bounded implementation issue before it receives a support
  claim.

## Validation and review triggers

- Verify that the documented generic fallback and configuration behavior remain
  unchanged when #96 lands.
- Revisit the target order if measured repository workloads, user requests, or
  a maintained integration provide stronger evidence for another language.
- Revisit semantic scope only when a concrete workflow requires language
  meaning beyond parser-selected spans.

## References

- [Issue #92](https://github.com/sebastian-software/ferrolex/issues/92)
- [Follow-up #96](https://github.com/sebastian-software/ferrolex/issues/96)
- [Source-code analysis](../source-code-analysis.md)
- [Source analysis epic #10](https://github.com/sebastian-software/ferrolex/issues/10)
