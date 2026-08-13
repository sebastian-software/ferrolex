# ADR-0002: Native-only, CPU- and memory-optimized engine

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

The engine targets CLIs, language servers, CI pipelines, and embedding into
native applications. Supporting browser/WebAssembly as a first-class target
would constrain exactly the techniques the performance goals depend on:
memory-mapped compiled dictionaries (no mmap in wasm), thread-based
parallelism (gated behind SharedArrayBuffer), dependency choices, and
data-structure layout. It would also add a permanent maintenance and testing
surface.

## Decision

The engine is native-only. Browser and WebAssembly deployment are not goals,
initial or otherwise.

The engine is consistently CPU-optimized with memory-saving operations
wherever feasible: cache-friendly contiguous layouts, allocation-free lookup
hot paths, compact encodings, and SIMD only where benchmarks prove a win
When memory savings and hot-path latency conflict, latency wins and
the trade-off is documented.

## Considered options

### WASM as first-class target (rejected)

No product need. Constrains mmap, threading, and dependency choices; adds a
build/test matrix the project does not want to carry.

### "Keep the door open" neutrality (rejected)

Soft portability constraints creep into design decisions even without a
declared target. An explicit non-goal keeps decisions honest.

### Native-only (chosen)

Maximizes the value of the compiled-dictionary and mmap strategy and keeps
the optimization space unconstrained.

## Consequences

- Reversal is expensive by design: a future WASM port would need to revisit
  the mmap-centric compiled format and threading model. This cost is accepted.
- `wasm32` compatibility must not be used as an argument in design or
  dependency reviews.
- Node.js and other non-Rust consumers are served through native bindings, not
  WASM.

## Validation and review triggers

- Reopen only for a concrete, funded product need for browser embedding.

## References

- [Compiled artifacts and performance epic](https://github.com/sebastian-software/ferrolex/issues/81)
- [Supported integrations epic](https://github.com/sebastian-software/ferrolex/issues/82)
