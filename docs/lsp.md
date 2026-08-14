# ferrolex language server

> **Prototype outside the current product scope.** ferrolex does not currently
> distribute or maintain an LSP release surface. Format-aware tools should call
> the Rust or Node.js engine after parsing their own documents. This document
> records the checked-in prototype until its code is removed or relocated.

`ferrolex-lsp` is a generic Language Server Protocol server over stdio. It is
editor-neutral: any LSP client can launch `cargo run -p ferrolex-lsp` during
development, or the built binary in a packaged integration.

## Capabilities

- full document synchronization with incremental-change application;
- `textDocument/publishDiagnostics` for unknown words, with UTF-16 LSP ranges;
- `textDocument/codeAction` quick fixes for bounded spelling suggestions;
- whole-identifier replacements with the existing case-preserving recombination;
- an inline `ferrolex:ignore` action using the configured comment prefix; and
- `ferrolex.addToDictionary`, which updates and, when configured, persists the
  user dictionary before diagnostics are refreshed.

`workspace/didChangeConfiguration` reloads the server configuration and
reanalyzes open documents. It does not depend on a particular editor extension.

## Configuration

Set this object either in `initializationOptions.ferrolex` or under the
`ferrolex` key of `workspace/didChangeConfiguration`:

```json
{
  "words": ["ferrolex", "Project"],
  "ignoredWords": ["generated"],
  "commentPrefix": "//",
  "userDictionaryPath": "/absolute/path/to/ferrolex-user-words.txt"
}
```

`words` is the small plain-word-list dictionary used by the generic spike;
production dictionary selection remains a separate integration decision.
`userDictionaryPath` uses ferrolex's deterministic plain-word-list format.

## Prototype status

The protocol server, diagnostics, quick fixes, user-dictionary flow,
configuration reload, and incremental document handling are checked-in
prototype code. They are not an active release surface or compatibility
promise. No versioned LSP artifacts or editor distribution are planned under
the current product focus. See
[ADR-0010](adr/0010-external-integration-support-tiers.md).
