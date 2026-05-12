# ADR 0004: Do Not Use Tree-Sitter In v0.1

## Status

Accepted

## Context

Tree-sitter would provide broad language coverage, but the first useful version
of Ferrolex should focus on the checking core, dictionary handling, tokenization,
and narrow source extraction.

## Decision

Ferrolex v0.1 will start with JSON extraction and OXC-based JavaScript and
TypeScript extraction. It will not include Tree-sitter or CodeSitter.

## Consequences

- The source adapter surface stays modular for future parser integrations.
- OXC extraction must remain generic and avoid Palamedes-specific behavior.
- Broader language support can be revisited after the checker core is useful.
