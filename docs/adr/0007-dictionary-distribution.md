# ADR-0007: Dictionary distribution via companion repository and releases

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

The core engine ships without dictionaries to stay license-clean (RFC §4.3).
Users still need a low-friction way to obtain compiled dictionaries, and
dictionary licenses (often GPL/LGPL/MPL or custom) must remain explicit and
separate from the engine's MIT OR Apache-2.0 licensing.

## Decision

Dictionaries are distributed through a companion repository
(`ferrolex-dictionaries`) that:

- fetches upstream Hunspell dictionaries,
- tracks and documents each dictionary's license,
- compiles them deterministically in CI (byte-identical, ADR-0006),
- publishes versioned compiled artifacts via releases.

A `ferrolex fetch <locale>` CLI command may build on this later. Compiled
artifacts embed their source and license metadata (RFC §12).

## Considered options

### No distribution initially (rejected)

Lowest effort, but every adopter re-solves the same problem; adoption
friction directly against the project's goals.

### Per-language crates with embedded dictionaries (rejected)

Convenient for Rust users, but mixes dictionary licenses into the crate
graph and fights crates.io size limits permanently.

### Companion repository with released artifacts (chosen)

Keeps the engine license-clean, makes dictionary licenses explicit per
artifact, and reuses deterministic compilation for cacheable, versioned
downloads.

## Consequences

- A second repository must be created and wired into CI once the compiler
  (Phase 5) exists; until then this decision has no implementation cost.
- License review becomes part of adding a dictionary to the companion repo.

## Validation and review triggers

- Reopen if artifact hosting via repository releases becomes a bottleneck
  (bandwidth, discoverability) — a registry/CDN would be the next step.

## References

- [rfc.md](../../rfc.md) §4.3, §12, §42
