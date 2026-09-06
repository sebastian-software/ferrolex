# Documentation index

This directory contains the durable product and compatibility documentation for
ferrolex. The repository root [README](../README.md) is the best starting
point for installation, command-line use, and the public Rust API.

## Product documentation

- [Dictionary fetching](dictionary-fetching.md) and
  [locale compatibility](locale-compatibility.md) describe the reviewed source
  catalog and its evidence boundary.
- [Hunspell import contract](hunspell-format.md),
  [affix semantics](affix-semantics.md), and
  [compound semantics](compound-semantics.md) define supported recognition
  behavior.
- [Suggestions](suggestions.md), [explanations](explanations.md),
  [compiled dictionary format](binary-format.md), and
  [Hunspell runtime cache](hunspell-runtime-cache.md) document engine APIs and
  artifacts.
- [Command-line workflow](command-line-workflow.md) records command-line
  options, output streams, and exit statuses.
- [Shared family tooling](family-tooling.md) defines the plain-text tokenizer
  boundary and the integration evaluation queue.
- [README and brand standard](readme-standard.md) records the generated family
  block, badges, tagline, and casing contract.
- [Compatibility reporting](compatibility.md) explains the format,
  recognition, morphology, and ecosystem evidence levels.
- [Compatibility fixtures](compatibility-fixtures.md),
  [robustness testing](robustness-testing.md), and
  [performance](performance.md) describe verification and measured limits.

## Prototype and integration history

Parser-backed source analysis and the integrations below are retained as
prototype history, not current ferrolex product commitments:

- [Source-code analysis](source-code-analysis.md)
- [Native integrations](integrations.md), [C FFI](ffi.md), [LSP](lsp.md), and
  [VS Code](vscode.md)
- [Bindings](bindings.md) and [neutral IR](neutral-ir.md)

The supported direct runtime integration is the Node.js package, which requires
Node.js 22.13 or newer. The C ABI and Python binding are evaluation prototypes;
the LSP and VS Code client are retained editor prototypes outside the current
product scope. See [ADR-0010](adr/0010-external-integration-support-tiers.md)
for the support tiers.

Architecture decisions live in the [ADR index](adr/README.md).
