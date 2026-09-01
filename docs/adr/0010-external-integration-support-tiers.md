# ADR-0010: Node.js is the first direct runtime integration

- Status: accepted
- Date: 2026-08-13
- Last updated: 2026-09-01
- Deciders: Ferrolex maintainers

## Context

The workspace contains prototypes for a C ABI, Node.js, Python, a generic LSP,
and Visual Studio Code. Treating every prototype as maintained created several
simultaneous API, packaging, platform, protocol, and CI commitments before the
core product boundary was settled.

Node.js is directly useful to the surrounding tool family and follows an
established packaging pattern used by sibling projects. LSP and editor
distribution add a separate end-user product, platform binaries, protocol
behavior, installation lifecycle, and editor support. Python and the C ABI do
not currently have a concrete consumer or distribution requirement.

## Decision

Node.js is the only selected direct non-Rust integration. Its API mirrors the
Rust engine's dictionary, checking, suggestion, and managed-acquisition model.
It remains pre-1.0 until an npm package, supported platform matrix, TypeScript
types, and clean-install verification exist.

The C ABI and Python packages are retained only as evaluation prototypes. The
generic LSP and Visual Studio Code client are outside the current product and
distribution scope. They do not define release artifacts or compatibility
promises. Existing workspace CI checks may remain temporarily while the
prototype code is evaluated for removal. They are excluded from default
workspace members and focused release compilation and tests, with a
path-filtered, manually dispatchable workflow retaining targeted implementation
evidence.

Document and language integrations belong to their owning tools and call the
Rust or Node.js API after parsing. They do not require a ferrolex-owned LSP.

## Consequences

- Maintainers have one direct binding to package and support first.
- LSP binaries, editor installation, protocol compatibility, and multi-platform
  smoke matrices are not release requirements.
- Prototype source can be removed independently after checking whether it still
  provides useful implementation evidence.
- Dictionaries remain caller-controlled and follow ADR-0007 in every runtime.

## Promotion criteria

Promote the Node.js binding when the package has a named npm owner, documented
TypeScript API, prebuilt binary policy, dictionary workflow, and clean-install
tests on its declared platforms.

Reconsider another direct binding only for a concrete consumer that cannot use
Rust or Node.js and supplies a credible packaging and maintenance owner.

## References

- [Native integrations](../integrations.md)
- [Node.js and deferred binding prototypes](../bindings.md)
- [Dictionary distribution](0007-dictionary-distribution.md)
