# Native integrations

ferrolex is a native Rust library and CLI first. Its core dictionary, text,
code-analysis, compiler, and suggestion crates are deliberately independent of
editor protocols and foreign-function runtimes.

RFC §48 marks an LSP as a desirable later-stage deliverable, not a requirement
for the initial core. The experimental [C ABI spike](ffi.md) now records a
narrow ownership, threading, error, and distribution contract for a
plain-word-list checker. It is feature-gated, unpublished, and has no
stability promise yet. Node.js, Python, and editor extensions remain deferred
until a consumer defines their corresponding contracts.

The present integration boundary is therefore the Rust workspace and the
`ferrolex` CLI. Both expose structured failures and deterministic output so an
LSP or native binding can be added without moving editor or FFI semantics into
the core crates.
