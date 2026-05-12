# ADR 0002: Use Lexios As The Brand Source Of Truth

## Status

Accepted

## Context

Ferrolex includes brand validation, but Sebastian Software already has Lexios as
the Brand ID schema and research workspace. Treating Lexios as an external
format or optional import would create two competing brand models.

## Decision

Ferrolex brand validation will be Lexios-based. `ferrolex-brand` is the runtime
checker layer for Lexios Brand ID data. If Ferrolex needs richer brand concepts,
those concepts should be developed in Lexios.

## Consequences

- Ferrolex should not add a separate brand guide schema.
- Lexios may need a language-neutral schema artifact for clean Rust
  consumption.
- Brand checker fixtures should use Lexios-shaped data.
