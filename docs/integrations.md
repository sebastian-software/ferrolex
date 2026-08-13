# Native integrations

ferrolex is a native Rust library and CLI first. Its core dictionary, text,
code-analysis, compiler, and suggestion crates remain independent of editor
protocols and foreign-function runtimes.

[ADR-0010](adr/0010-external-integration-support-tiers.md) records the
current product tiers. The C ABI, Node.js, Python, generic stdio LSP, and VS
Code client are all **maintained experimental**. Their source contracts and CI
checks are maintained, but none yet makes a stable ABI, package, protocol, or
Marketplace compatibility promise.

The generic [LSP](lsp.md) is the first selected external distribution surface:
[Issue #91](https://github.com/sebastian-software/ferrolex/issues/91) will
deliver versioned GitHub Release artifacts and clean-install verification for
its declared native platform matrix. Until then it is source-built. The
[Visual Studio Code client](vscode.md) remains a thin, locally packaged client
that resolves a user-supplied server; Marketplace publication remains deferred
until that LSP artifact and update contract exists.

The [C ABI spike](ffi.md) and [Node.js/Python binding spikes](bindings.md) are
workspace-only experiments. They accept caller-provided plain-word-list data
and neither bundle nor fetch dictionaries. All dictionary acquisition and
redistribution decisions remain subject to ADR-0007.
