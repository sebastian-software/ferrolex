# Native integrations

ferrolex is a native Rust library and CLI first. Its core dictionary, text,
code-analysis, compiler, and suggestion crates are deliberately independent of
editor protocols and foreign-function runtimes.

The [supported integrations epic](https://github.com/sebastian-software/ferrolex/issues/82)
tracks the decision to promote an LSP or binding beyond the initial core. The
experimental [C ABI spike](ffi.md) now records a
narrow ownership, threading, error, and distribution contract for a
plain-word-list checker. It is feature-gated, unpublished, and has no
stability promise yet. The [Node.js and Python binding spikes](bindings.md)
likewise remain unpublished while their packaging and compatibility contracts
are evaluated. The generic [stdio language server](lsp.md) and its thin
[Visual Studio Code client](vscode.md) ship separately from the core crates.
The client launches a user-resolved server binary and maps workspace settings;
Marketplace publication remains deferred until portable server distribution is
defined.

The present integration boundary is therefore the Rust workspace and the
`ferrolex` CLI. Both expose structured failures and deterministic output so an
LSP or native binding can be added without moving editor or FFI semantics into
the core crates.
