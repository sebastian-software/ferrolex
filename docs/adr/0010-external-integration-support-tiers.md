# ADR-0010: External integration support tiers and first distribution surface

- Status: accepted
- Date: 2026-08-13
- Last updated: 2026-08-13
- Deciders: Ferrolex maintainers

## Context

RFC §§28–32, 48–49, and 56 describe native embedding, bindings, a language
server, and editor clients as important future directions. The Phase 8 work in
#13 produced useful C ABI, Node.js, Python, LSP, and Visual Studio Code
prototypes. A successful prototype and its CI test are not, by themselves, a
promise that users can install, update, or safely depend on the surface.

The core remains a native Rust library and CLI. It does not bundle dictionary
data (ADR-0007), and its public API is still pre-1.0. This record makes the
product decision without implying that an unshipped package or binary already
exists.

## Decision

All five surfaces are **maintained experimental**. This means the named owner
keeps its documented source contract, CI coverage, and decision record current,
but may make a breaking change in a minor release with release notes. It is not
a stable ABI, package, protocol, or Marketplace compatibility promise. A
surface becomes **supported** only when its declared release channel and clean
consumer verification exist. No surface is a no-go: the deferred work below is
intentional, not abandoned.

| Surface | Tier and owner | Compatibility promise | Dictionary-distribution model | Target platforms | Release channel | Verification responsibility |
| --- | --- | --- | --- | --- | --- | --- |
| C ABI | Maintained experimental — core maintainers | The checked-in header, opaque-handle ownership, status values, and UTF-8 rules are reviewed together; no cross-release ABI stability is promised. | Caller supplies UTF-8 plain-word-list bytes. No bundled, fetched, or redistributed dictionaries. | Source-built and CI-verified on Linux x86_64; other targets have no support commitment. | Workspace/source only; no released library, header package, or registry artifact. | Core maintainers run the `c-abi` tests and header synchronization check. |
| Node.js | Maintained experimental — binding maintainers | The documented `Checker` spike is exercised, but JavaScript/TypeScript API, binary, and package compatibility may change without an npm stability promise. | Caller supplies plain-word-list text. No package carries dictionary data. | Source-built and CI-verified on Linux x86_64 with Node.js 20. | Workspace/source only; no npm package or prebuilt binary. | Binding maintainers run the native-extension import and lookup benchmark in CI. |
| Python | Maintained experimental — binding maintainers | The documented `Checker` spike is exercised, but Python API, ABI, wheel, and package compatibility may change without a PyPI stability promise. | Caller supplies plain-word-list text. No wheel carries dictionary data. | Source-built and CI-verified on Linux x86_64 with the CI `python3`. | Workspace/source only; no wheel or PyPI package. | Binding maintainers run the real extension import and lookup benchmark in CI. |
| LSP | Maintained experimental — LSP maintainers | The documented stdio methods, configuration, diagnostics, and code actions are maintained; protocol and configuration changes may occur with release notes until a supported server release exists. | The server uses caller-configured plain-word-list and user-dictionary paths. It neither bundles nor implicitly fetches dictionaries. | First release targets: macOS arm64, Ubuntu x86_64, and Windows x86_64. Until #91 lands, only source-built use is verified. | First selected external distribution surface: versioned `ferrolex-lsp` artifacts on GitHub Releases, delivered by #91. | LSP maintainers own Rust protocol tests now; #91 must add packaging, checksum, and clean-install stdio smoke tests for every target. |
| Visual Studio Code | Maintained experimental — editor maintainers | The thin-client settings mapping and command resolution are maintained; there is no Marketplace, bundled-server, or extension update compatibility promise. | The client does not carry dictionaries; it passes caller-selected settings and paths to the LSP. | VS Code `^1.85.0` on platforms where the user resolves a compatible server. | Workspace source and locally built VSIX only; no Marketplace publication. | Editor maintainers own extension tests and local-package checks in CI. |

### First distribution surface

The generic LSP is the first external surface selected for distribution because
one server release can serve editor-neutral clients while preserving the
native-first core. [Issue #91](https://github.com/sebastian-software/ferrolex/issues/91)
is the bounded delivery issue for versioned LSP artifacts, documented lifecycle,
and per-platform clean-consumer verification. It does not publish the C ABI,
Node.js, Python, or VS Code surfaces.

### Deferral criteria

- Promote the C ABI only after a real C consumer validates packaging and
  ownership, the Rust API it wraps is stable, and the project chooses a
  versioned library/header distribution and platform matrix.
- Promote Node.js only after an npm owner, TypeScript API policy, prebuilt
  binary matrix, package lifecycle, and clean-install verification are in
  place.
- Promote Python only after a PyPI owner, interpreter/`abi3` and wheel matrix,
  package lifecycle, and clean-install verification are in place.
- Publish the VS Code extension to Marketplace only after #91 has delivered
  portable server artifacts, the extension resolves and updates those artifacts
  on its declared platforms, and Marketplace install/update verification exists.
- Reassess every tier when a concrete consumer requires a stable promise or a
  distribution channel. Do not infer support from a prototype, CI job, or local
  build alone.

## Consequences

- Maintainers may continue to improve the existing spikes without creating an
  accidental package or ABI commitment.
- Users can distinguish a locally reproducible experiment from the first
  planned installable integration.
- Dictionary licensing and acquisition remain governed by ADR-0007 for every
  external surface.

## References

- [Native integrations](../integrations.md)
- [Experimental C ABI](../ffi.md)
- [Experimental Node.js and Python bindings](../bindings.md)
- [Language server](../lsp.md)
- [Visual Studio Code integration](../vscode.md)
- [Supported integrations epic](https://github.com/sebastian-software/ferrolex/issues/82)
- [First external integration delivery](https://github.com/sebastian-software/ferrolex/issues/91)
- [Historical prototype epic](https://github.com/sebastian-software/ferrolex/issues/13)
