# Native integrations

ferrolex is a native Rust library and CLI first. Its core dictionary, text,
code-analysis, compiler, and suggestion crates are deliberately independent of
editor protocols and foreign-function runtimes.

RFC §48 marks an LSP as a desirable later-stage deliverable, not a requirement
for the initial core. Node.js/Python bindings and a C ABI are likewise deferred
until a consumer defines their ownership, threading, error, and artifact
distribution contracts. No compatibility promise for those integrations is
made before that design work.

The present integration boundary is therefore the Rust workspace and the
`ferrolex` CLI. Both expose structured failures and deterministic output so an
LSP or native binding can be added without moving editor or FFI semantics into
the core crates.
