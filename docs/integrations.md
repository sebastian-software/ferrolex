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
compilation and tests. A path-filtered prototype workflow checks them when their
own manifests, source, lockfile, or workflow changes and can also be started
manually. Changes to the shared core, code-analysis, or suggestion APIs also
trigger it so retained prototypes cannot silently drift from their dependencies.

Format-aware integration happens in the owning tool:

- Ferromark selects prose from Markdown.
- Ferrocat selects translatable content from PO catalogs.
- OXC selects relevant text from TypeScript source.

Those tools call ferrolex after parsing. Ferrolex deliberately does not embed
their parsers, own their configuration, or define editor-protocol behavior.

All integrations use caller-controlled dictionaries. Verified acquisition and
local caching remain governed by [ADR-0007](adr/0007-dictionary-distribution.md).
