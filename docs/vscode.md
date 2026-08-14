# Visual Studio Code integration

> **Prototype outside the current product scope.** There is no planned
> Marketplace or bundled-LSP distribution. This document records the existing
> client prototype until its code is removed or relocated.

The VS Code prototype lives in
[`editors/vscode/ferrolex`](../editors/vscode/ferrolex). It is deliberately
thin: it starts the generic [`ferrolex-lsp`](lsp.md) server over stdio and
maps VS Code's `ferrolex.*` settings to the server's initialization and
configuration protocol.

## What users get

Once `ferrolex-lsp` is on `PATH` (or `ferrolex.lsp.command` names its absolute
path), opening a file starts the server. Unknown words appear as VS Code
diagnostics. The server's suggestion, add-to-dictionary, and inline-ignore
code actions appear in the editor's standard **Quick Fix** menu.

The extension scopes its dictionary configuration to the workspace resource,
so a repository can keep its accepted and ignored words alongside its VS Code
settings. The optional `ferrolex.userDictionaryPath` remains a user-chosen
plain-word-list file and is persisted by the server.

## Local prototype packaging

The extension has no Marketplace or bundled-server compatibility promise. Its
existing source can still be packaged locally for evaluation with:

```sh
cd editors/vscode/ferrolex
npm install
npx @vscode/vsce package
```

Marketplace publication and automatic LSP acquisition are not on the current
roadmap. See [ADR-0010](adr/0010-external-integration-support-tiers.md).
