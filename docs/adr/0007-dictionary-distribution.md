# ADR-0007: Verified upstream sources and local dictionary caches

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-12
- Deciders: Sebastian Werner

## Context

The core engine ships without dictionaries to stay license-clean (RFC §4.3).
Users still need a low-friction way to obtain compiled dictionaries, and
dictionary licenses (often GPL/LGPL/MPL or custom) must remain explicit and
separate from the engine's MIT OR Apache-2.0 licensing.

## Decision

Dictionaries are not redistributed by ferrolex. The optional `ferrolex
dictionary fetch <locale>` command downloads the reviewed upstream `.aff` and
`.dic` files directly into a caller-selected local cache. Every catalog entry
pins an immutable upstream revision, paths, source-byte SHA-256 digests,
encoding, SPDX license expression, and the upstream license notice.

The installer verifies bytes before its cache write. It never downloads during
normal checking, compilation, tests, or CI; it follows no redirects and
performs no background updates. `dictionary install` may then produce a local
provenance-bound runtime cache from the verified sources.

Compiled artifacts remain a downstream distribution choice. A product that
redistributes one must retain and comply with that locale's recorded license
notice; ferrolex does not publish a shared artifact registry.

## Considered options

### No distribution initially (rejected)

Lowest effort, but every adopter re-solves the same problem; adoption
friction directly against the project's goals.

### Per-language crates with embedded dictionaries (rejected)

Convenient for Rust users, but mixes dictionary licenses into the crate
graph and fights crates.io size limits permanently.

### Direct verified upstream sources with a local cache (chosen)

Keeps the engine license-clean without adding a second release pipeline or
redistributing third-party content. The explicit cache and immutable catalog
make source provenance inspectable while leaving each consuming product in
control of its local data and distribution obligations.

### Companion repository with released artifacts (deferred)

Would improve offline bootstrap for products that need a curated artifact
channel, but would also make ferrolex responsible for hosting, release
operations, artifact provenance, and redistribution review. No such product
requirement exists yet.

## Consequences

- The catalog is a reviewed source manifest, not a claim that all LibreOffice
  dictionaries share one license.
- Each install is an explicit network action and needs a caller-supplied cache
  location; normal runtime behavior remains offline and deterministic.
- Adding or updating a locale requires review of its immutable source revision,
  digests, license identity, encoding, and license notice.

## Validation and review triggers

- Reopen when a consuming product needs ferrolex-maintained precompiled
  artifacts, offline bootstrap from a shared channel, or a release cadence
  independent of the CLI. That review must decide whether a companion
  repository, registry, or CDN can carry the required per-locale provenance and
  license evidence.

## References

- [Dictionary fetching](../dictionary-fetching.md)
- [Locale compatibility](../locale-compatibility.md)
- [rfc.md](../../rfc.md) §4.3, §12, §42
