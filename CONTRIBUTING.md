# Contributing to ferrolex

## Language and commits

All durable repository artifacts use US English. Commit messages follow the
[Conventional Commits](https://www.conventionalcommits.org/) specification;
pull-request titles are checked in CI. Release Please creates one product
release PR for the whole Rust workspace. All public workspace crates share a
version and release record through the root `ferrolex` umbrella package.

## Code provenance

ferrolex is independently implemented and licensed `MIT OR Apache-2.0`.
Studying documented formats, observable behavior, concepts, and existing
implementations is permitted. Copying, file-by-file translation, mechanical
conversion, and side-by-side porting of incompatible implementations are not.

## Dependency policy exceptions

`cargo deny check` enforces dependency licenses, RustSec advisories, duplicate
versions, banned dependency rules, and trusted package sources in CI. Keep the
default policy narrow: dependencies must be distributable under `MIT OR
Apache-2.0` and originate from crates.io.

If a necessary dependency does not pass, do not suppress the check in CI. Open
an issue that records the crate and version, why it is needed, the relevant
license or advisory assessment, the maintainer who approved it, and an expiry
or removal plan. Add the smallest version-scoped entry to `deny.toml` with that
issue URL as its `reason`; remove it once no longer needed.

Spellbook must not be used as porting material. AI-assisted contributions are
reviewed for obvious structural closeness to known implementations and should
be prompted against ferrolex-owned behavior documentation rather than asking
for reproductions of other implementations.

See [ADR-0001](docs/adr/0001-code-provenance-policy.md) for the rationale and
the RFC for project requirements.
