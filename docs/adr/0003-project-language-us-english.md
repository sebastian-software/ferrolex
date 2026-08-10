# ADR-0003: Project language is US English

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

The maintainers are German speakers, but the project is public open source
aimed at an international audience of Rust developers and commercial
adopters. Mixed-language repositories raise contribution friction and read as
unpolished during due diligence.

## Decision

All durable repository artifacts are written in US English: identifiers,
comments, documentation, commit messages, decision records, issues, and pull
requests. US spelling is used consistently (e.g., *analyzer*, *color*,
*behavior*, *license* as noun and verb).

Informal maintainer chat may be German; anything that lands in the repository
is US English.

## Consequences

- Reviews flag non-English or mixed-spelling artifacts as defects.
- German-language domain examples (e.g., compound words like
  `Haustürschlüssel` in tests and docs) remain welcome as *data* — the
  surrounding prose stays English.

## References

- [rfc.md](../../rfc.md) §60
