# ADR-0002: Native-only, CPU- and memory-optimized engine

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-14
- Deciders: Sebastian Werner

## Context

Ferrolex targets native Rust applications, its CLI, and native Node.js
bindings. Supporting browser/WebAssembly as a first-class target would add a
permanent maintenance and testing surface without serving the focused product
contract.

## Decision

The engine is native-only. Browser and WebAssembly deployment are not goals,
initial or otherwise.

Performance work is benchmark-driven. Cache-friendly layouts, compact
encodings, memory mapping, threading, and SIMD are implementation options, not
product promises; they are adopted only when measurements justify their cost.

## Considered options

### WASM as first-class target (rejected)

No product need. Adds a build/test matrix and portability constraints the
project does not want to carry.

### "Keep the door open" neutrality (rejected)

Soft portability constraints creep into design decisions even without a
declared target. An explicit non-goal keeps decisions honest.

### Native-only (chosen)

Keeps the implementation and optimization space unconstrained.

## Consequences

- A future WASM port would need a separate product decision and validation
  surface.
- `wasm32` compatibility must not be used as an argument in design or
  dependency reviews.
- Node.js and other non-Rust consumers are served through native bindings, not
  WASM.

## Validation and review triggers

- Reopen only for a concrete, funded product need for browser embedding.

## References

- [Architecture](../../ARCHITECTURE.md)
- [Supported integrations epic](https://github.com/sebastian-software/ferrolex/issues/82)
- [ADR-0010: Node.js is the first direct runtime integration](0010-external-integration-support-tiers.md)
