# Visual Studio Code integration

The maintained VS Code client lives in
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

## Binary and distribution decision

**Go:** maintain the extension source, its settings contract, and reproducible
local package verification. Install its JavaScript dependency and build a local
VSIX with:

```sh
cd editors/vscode/ferrolex
npm install
npx @vscode/vsce package
```

**No-go for Marketplace publication (for now):** the extension does not bundle
or download a `ferrolex-lsp` binary. Publishing before signed, versioned,
platform-specific server artifacts and their update policy exist would leave
users with an extension that cannot reliably start its server. The command
setting is therefore the explicit, portable resolution mechanism until that
release contract is implemented.
