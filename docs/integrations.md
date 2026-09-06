# Native integrations

ferrolex is a Rust spell-checking engine with one selected direct runtime
integration: Node.js. The Node package exposes the same dictionary,
recognition, suggestion, and managed-acquisition concepts as the Rust API. It
does not fork recognition behavior or bundle dictionary data.

[ADR-0010](adr/0010-external-integration-support-tiers.md) records this focus.
The checked-in C ABI and Python binding remain evaluation prototypes without a
current distribution or compatibility commitment. The generic LSP and Visual
Studio Code client are also outside the current product scope; their presence
in the workspace does not make them maintained release surfaces. Those crates
are excluded from the workspace's default members and focused release
compilation and tests. The Node.js crate is the exception: it is `publish =
false` for crates.io because its release artifact is npm, but it remains in
`default-members` so its Rust build and Node package gate run with the product.
A path-filtered prototype workflow checks the other retained prototypes when
their own manifests, source, lockfile, or workflow changes and can also be
started manually. Changes to the shared core, code-analysis, or suggestion APIs
also trigger it so retained prototypes cannot silently drift from dependencies.

Format-aware integration is designed to happen in the owning tool:

- [ferromark](https://github.com/sebastian-software/ferromark) selects prose
  from Markdown.
- [ferrocat](https://github.com/sebastian-software/ferrocat) selects
  translatable content from PO catalogs.
- [OXC](https://oxc.rs) selects relevant text from TypeScript source.

Those tools are designed to call ferrolex after parsing; no sibling adapter is
part of the current support tier. ferrolex deliberately does not embed their
parsers, own their configuration, or define editor-protocol behavior.

For the shared plain-text boundary, use [`ferrolex-text`](family-tooling.md).
It is the family tokenizer for extracted prose and catalog strings; the
consuming tool still owns field selection, source locations, and ignore rules.
The initial catalog and documentation evaluations are tracked for Palamedes /
ferrocat and Ferramenta in the [family tooling contract](family-tooling.md).

All integrations use caller-controlled dictionaries. Verified acquisition and
local caching remain governed by [ADR-0007](adr/0007-dictionary-distribution.md).
