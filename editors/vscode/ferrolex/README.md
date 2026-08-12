# Ferrolex for Visual Studio Code

This extension launches the generic `ferrolex-lsp` server over stdio. Its
diagnostics and quick fixes are shown by VS Code through the Language Server
Protocol.

## Prerequisite

Install `ferrolex-lsp` on `PATH`, or set `ferrolex.lsp.command` to an absolute
path to the server binary. During development it can be built with:

```sh
cargo build -p ferrolex-lsp
```

## Settings

- `ferrolex.lsp.command` (default `ferrolex-lsp`) selects the server binary.
- `ferrolex.dictionary.words` adds accepted words.
- `ferrolex.ignoredWords` ignores words without persisting them.
- `ferrolex.commentPrefix` controls the inline-ignore quick fix.
- `ferrolex.userDictionaryPath` selects the optional persistent plain-word-list
  dictionary.

The settings are sent when the extension starts and updated through the LSP
configuration notification when they change. Use **Ferrolex: Restart Language
Server** after changing the server command itself.

## Packaging status

The extension source and local package contract are maintained in this
repository. Marketplace publication is intentionally deferred: ferrolex does
not yet distribute signed, versioned `ferrolex-lsp` binaries for every VS Code
platform. Until that release contract exists, package a local VSIX with
`npx @vscode/vsce package` after installing dependencies, and configure the
server command explicitly.
