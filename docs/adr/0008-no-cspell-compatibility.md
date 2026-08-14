# ADR-0008: Own directive format, no cspell compatibility promises

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

cspell is the dominant developer-focused spell checker and defines a family
of inline directives (`cspell:ignore`, `cspell:disable`), a configuration
format (`cspell.json`), and the MIT-licensed cspell-dicts vocabulary
collection. Compatibility with any of these would lower migration cost but
couple ferrolex to a third-party format that evolves outside its control.

## Decision

ferrolex defines its own inline directive format (`ferrolex:ignore`,
`ferrolex:disable` / `ferrolex:enable`) and makes no compatibility promises
for cspell directives or `cspell.json` configuration.

The cspell-dicts word lists remain a possible *data source* for a plain
word-list importer — that is ordinary dictionary import, not format
compatibility, and carries no behavioral coupling.

## Considered options

### cspell directive and dictionary compatibility (rejected)

Was the reviewer recommendation as an adoption lever, but accepted-against:
it creates a permanent obligation to track a moving third-party format and
blurs the project's own identity.

### Full compatibility including cspell.json (rejected)

Largest migration lever, largest coupling; the configuration surface is the
fastest-moving part of cspell.

### Own format only (chosen)

Clean cut, no external format obligations, directives carry the project's
name.

## Consequences

- Existing cspell projects must translate their inline directives when
  migrating; a one-shot migration script could soften this without creating
  a compatibility promise.
- Documentation should state the non-goal explicitly so compatibility
  expectations don't accumulate in the issue tracker.

## Validation and review triggers

- Reopen if adoption feedback shows cspell migration friction is a real
  barrier in practice.

## References

- [Keep document parsing outside ferrolex](0009-language-aware-source-analysis.md)
