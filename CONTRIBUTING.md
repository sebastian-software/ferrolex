# Contributing to ferrolex

## Language and commits

All durable repository artifacts use US English. Commit messages follow the
[Conventional Commits](https://www.conventionalcommits.org/) specification;
pull-request titles are checked in CI. Release Please creates one product
release PR for the whole Rust workspace. All public workspace crates share a
version and release record through the root `ferrolex` umbrella package. The
`Release version contract` CI gate verifies member versions, internal Cargo
requirements, the release manifest, and the explicit Cargo-workspace release
plugin configuration on every change.

## Developing

ferrolex supports Rust 1.88 and later. Install the pinned MSRV when you need
to verify it locally, then run the same core checks as CI:

```sh
rustup toolchain install 1.88
cargo +1.88 fmt --all -- --check
cargo +1.88 clippy --workspace --all-targets -- -D warnings
cargo +1.88 test --workspace
RUSTDOCFLAGS="-D warnings" cargo +1.88 doc --workspace --no-deps
```

The real-world Hunspell fixture suite is opt-in because it needs separately
obtained, licensed dictionary sources; see
[Compatibility fixtures](docs/compatibility-fixtures.md). The `scripts/`
directory contains the compatibility-fixture downloader and README-status
generator used by CI, plus opt-in Node.js and Python binding benchmarks.

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
issue URL and rationale in a nearby configuration comment (license exceptions
do not support a `reason` field); remove it once no longer needed.

### Reviewed cargo-deny license exceptions

The following crate-and-version exceptions were reviewed in [#84][issue-84].
They are not distribution-wide license allowances and must be revisited with
every dependency upgrade; remove an exception when the dependency is removed
or its license expression no longer needs it.

| Crate | SPDX term | Why it is needed | Removal plan |
| --- | --- | --- | --- |
| `cbindgen` 0.29.4 | `MPL-2.0` | Build-time generator for Ferrolex's FFI C header; its sources are not shipped in the Ferrolex distribution. | Reassess at every `cbindgen` upgrade; remove if FFI header generation no longer uses it. |
| `unicode-ident` 1.0.24 | `Unicode-3.0` | Transitive Unicode identifier tables used by Rust procedural-macro tooling; the crate otherwise declares `MIT OR Apache-2.0`. | Reassess at every `unicode-ident` upgrade; remove if its license expression no longer includes this term. |

[issue-84]: https://github.com/sebastian-software/ferrolex/issues/84

Spellbook must not be used as porting material. AI-assisted contributions are
reviewed for obvious structural closeness to known implementations and should
be prompted against ferrolex-owned behavior documentation rather than asking
for reproductions of other implementations.

See [ADR-0001](docs/adr/0001-code-provenance-policy.md) for the rationale and
the [GitHub delivery epics](https://github.com/sebastian-software/ferrolex/issues?q=is%3Aissue%20label%3Aepic)
for tracked requirements.
