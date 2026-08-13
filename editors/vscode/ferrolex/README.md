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

This is a **maintained experimental** extension: its source and local-package
contract are maintained, but it has no Marketplace or bundled-server
compatibility promise. Marketplace publication is deferred until the versioned
`ferrolex-lsp` release artifacts and portable update contract selected in
[ADR-0010](../../../docs/adr/0010-external-integration-support-tiers.md) are
delivered by [Issue #91](https://github.com/sebastian-software/ferrolex/issues/91).
Until then, package a local VSIX with `npx @vscode/vsce package` after
installing dependencies and configure the server command explicitly.
