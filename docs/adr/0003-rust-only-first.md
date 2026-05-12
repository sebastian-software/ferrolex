# ADR 0003: Keep The Initial Implementation Rust-Only

## Status

Accepted

## Context

Node bindings are useful for JavaScript and TypeScript tooling, but the Rust API,
tokenizer behavior, dictionary format, and Lexios brand validation model are not
settled yet.

## Decision

Ferrolex will start as a Rust-only workspace. Node bindings can be added after
the Rust API and runtime dictionary format have practical fixtures and
downstream use.

## Consequences

- The initial workspace does not include napi-rs or Neon.
- API design can focus on Rust correctness and performance first.
- Node packaging remains a later integration layer.
