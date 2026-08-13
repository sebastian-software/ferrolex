# ADR-0001: Pragmatic code provenance policy

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

The engine is an independent implementation licensed MIT OR Apache-2.0 in a
space dominated by copyleft implementations: Hunspell (GPL/LGPL/MPL
tri-license), Nuspell (LGPL), and Spellbook (MPL-2.0, a self-described Rust
rewrite of Nuspell). The credibility of the permissive-license claim is one of
the project's core differentiators — its audience is commercial adopters doing
license due diligence and an open-source community judging independence, not a
courtroom.

A maximal clean-room process (implementers never read competing source) is
neither legally required nor practically enforceable, especially with
AI-assisted development where models have been trained on existing
implementations. Copyright protects expression, not ideas: algorithms, file
formats, and observed behavior are free to reimplement (EU: SAS v. World
Programming; Germany: § 69a UrhG). The realistic risks are reputational and
adoption-related, not litigation.

## Decision

Clean provenance is treated as a product feature and enforced pragmatically:

- Studying existing implementations — including reading their source code —
  is permitted and encouraged.
- Copying, file-by-file translation, mechanical conversion, and side-by-side
  porting of code from incompatible implementations are prohibited.
- **Spellbook is explicitly off-limits as porting material.** As a Rust
  rewrite of Nuspell, Rust-to-Rust code proximity to it is both the most
  likely temptation and the most easily detected.
- AI-generated code is treated as a contribution of unknown provenance:
  reviewed for obvious structural closeness before merging, with prompts
  phrased against project-owned behavior documentation rather than requesting
  reproductions of specific implementations.
- The hard areas (affix and compound semantics) are specified in project-owned
  behavior documentation, which implementation and tests are written against.

## Considered options

### Strict clean-room (rejected)

Implementers barred from reading competing source. Legally unnecessary,
unenforceable in practice, high contributor friction, and incompatible with
AI-assisted development.

### No policy (rejected)

Undermines the project's main differentiator: an unverifiable MIT/Apache claim
is worth little to commercial adopters, and a public "relicensed rewrite"
accusation would hurt adoption regardless of legal merit.

### Pragmatic middle ground (chosen)

Matches the actual legal boundary (expression vs. ideas), keeps contribution
friction low, and concentrates strictness on the one vector that is both
tempting and detectable (Spellbook).

## Consequences

- `CONTRIBUTING.md` must carry the policy text.
- Code review includes a provenance check for suspicious structural closeness.
- Behavior documentation must exist before the hardest compatibility work
  starts, and serves as the implementation reference.
- The policy is a public trust artifact: it is documented visibly, not only
  internally.

## Validation and review triggers

- Reopen if a provenance claim is publicly challenged or legally asserted.
- Reopen if relevant case law on AI-generated code or non-literal copying
  changes materially.

## References

- [Compatibility delivery epic](https://github.com/sebastian-software/ferrolex/issues/80)
- Spellbook: <https://github.com/helix-editor/spellbook> (MPL-2.0)
