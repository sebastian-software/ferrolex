# Ferrolex for Visual Studio Code

> Prototype only. This extension is outside ferrolex's current product and
> distribution scope and is not planned for Marketplace publication.

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

There is no Marketplace or bundled-server compatibility promise. For local
evaluation only, package a VSIX with `npx @vscode/vsce package` after installing
dependencies and configure the server command explicitly. See
[ADR-0010](../../../docs/adr/0010-external-integration-support-tiers.md).
