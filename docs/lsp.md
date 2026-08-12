# ferrolex language server

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

## Scope decision

This is a **go for a maintained generic LSP implementation**. The protocol
server, diagnostics, quick fixes, user-dictionary flow, configuration reload,
and incremental document handling ship in the workspace. Editor packaging is
intentionally separate and is tracked by #67.
