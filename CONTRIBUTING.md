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

## Releasing

Merging the Release Please PR creates the GitHub release and tag. The release
workflow then runs the full Hunspell compatibility scorecard and, only after it
passes, publishes the public workspace crates to crates.io in dependency order.
The publish step exchanges the workflow's OIDC identity for a short-lived
crates.io Trusted Publishing token. Configure the repository and exact release
workflow as a trusted publisher for each of the nine public ferrolex crates;
no long-lived `CARGO_REGISTRY_TOKEN` secret is required.

Publishing is resumable: the script skips an exact crate version already on
crates.io and waits for every upload to reach Cargo's index before continuing.
After a failed publish job, rerun that job rather than changing the release tag.
The release workflow builds the eight declared Node.js platform packages,
publishes them with npm Trusted Publishing, and publishes `@ferrolex/node`
last. Configure the exact `release-please.yml` workflow as a trusted publisher
for the root package and every `@ferrolex/node-*` package before the first npm
release. The experimental FFI, Python, and LSP packages remain explicitly
unpublished.

## Developing

### Rust toolchain and MSRV

`workspace.package.rust-version` in the root `Cargo.toml` is the single source
of truth for the MSRV. The version is raised only when a change needs a newer
compiler, and it is never more than four stable releases behind the current
stable Rust. Two surfaces cannot read the manifest at use time: the pinned
`rust-toolchain.toml` that rustup consumes and the README badge. Raise the MSRV
in `Cargo.toml`, then run
`python3 scripts/workspace-rust-version.py --sync`; the matching `--check` runs
in `just gate` and in CI and fails on drift.

The pinned `rust-toolchain.toml` selects the supported toolchain locally; run
the same core checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

These commands mirror the Rust gate in the [CI workflow](.github/workflows/ci.yml):

- Workspace lints enable `clippy::pedantic` as a warning in the
  [workspace lint configuration](Cargo.toml#L103-L105), while `-D warnings`
  promotes every Clippy warning to a failure.
- `RUSTDOCFLAGS="-D warnings"` applies the same fail-on-warning policy to
  rustdoc, including missing or malformed documentation.
- Pull-request titles must use the Conventional Commits format; the
  [title workflow](.github/workflows/conventional-commits.yml) checks the
  title independently of the commit messages.

The repository also provides a tiered local runner when
[just](https://github.com/casey/just) is installed:

```sh
just quick
just gate
```

`quick` is the short feedback loop for formatting, the workspace clippy
configuration promoted to errors, and focused product-crate tests. `gate`
adds the workspace documentation and tests, benchmark compilation, the
approved suggestion-quality regression, cargo-deny, release-contract checks,
and package validation. It intentionally does not download licensed fixtures,
install tools, access registries, or require publishing credentials; those
network- and credential-dependent workflows remain explicit.

External and internal dependency versions are declared once in the root
`[workspace.dependencies]` table. Member manifests inherit those entries so a
dependency upgrade updates one policy location and keeps the workspace graph
aligned.

The real-world Hunspell fixture suite is opt-in because it needs separately
obtained, licensed dictionary sources; see
[Compatibility fixtures](docs/compatibility-fixtures.md). The `scripts/`
directory contains the compatibility-fixture downloader and README-status
generator used by CI, plus opt-in Node.js and Python binding benchmarks.

### Coverage gate

The `Rust coverage` job in the [CI workflow](.github/workflows/ci.yml) runs the
workspace test suite under instrumentation and fails when line coverage falls
below the threshold. That threshold is declared once, as the job's
`COVERAGE_MIN_LINES` environment variable; the README badge and this document
describe it but never set it. Coverage is enforced by this repository's own CI,
with no external coverage service and no token secret. Every run, including a
failing one, appends `Line coverage: X% (gate: ≥ N%)` to the workflow run
summary.

Run the same check locally with
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) installed and the
`llvm-tools-preview` component added:

```sh
just coverage
```

The recipe reads the threshold from the workflow, so it enforces exactly what
CI enforces. It stays out of `just gate` because it needs a tool the gate does
not otherwise install.

### Node workspaces and org standards

ferrolex is a Rust repository with no root `package.json`, and two Node
projects it authors itself: `crates/ferrolex-node`, which publishes
`@ferrolex/node`, and `editors/vscode/ferrolex`, the prototype editor
extension. Both are declared in `.repometa.json#workspaces`, so
[`@sebastian-software/standards`](https://github.com/sebastian-software/standards)
writes the org's Node configuration into those two directories: the managed
`.oxfmtrc.json` plus the seeded `eslint.config.ts`, `oxlint.config.ts`,
`tsconfig.json` and `cspell.json`. Run the formatter from inside the workspace
so its own config is the one that applies, not from the repository root. The
generated `npm/` sidecar under `crates/ferrolex-node` is not a workspace and is
not declared.

`crates/ferrolex-node/.prettierignore` keeps the formatter away from artifacts
this repository generates rather than writes: napi-rs regenerates `index.js`
and `index.d.ts` on every build and CI asserts they are unchanged, and the
package README carries the generated family block. oxfmt discovers that file on
its own; never add repository-specific ignores to the managed `.oxfmtrc.json`.

The `Standards drift` job in the [CI workflow](.github/workflows/ci.yml) runs
`standards check` from a version pinned in the workflow, because a repository
without a root lockfile has nowhere else to hold it. Renovate raises the pin
through the custom manager in [`renovate.json`](renovate.json); raise it and run
`standards apply` in the same pull request, or the stamp and the CLI that
checks it disagree.

### README family block

The root README is generated by native mdtheme from `README.md.src`. Sebastian
Software is the outer frame and Ferramenta the inner frame. Edit project prose
in the source, then run `mise run readme:write`; `mise run readme:check` checks
the entire result. See [README themes](docs/readme-theme.md) for installation,
CI, and the pre-push command. Standards 0.11.0 explicitly delegates README
ownership to mdtheme and does not append a company footer.

Published subpackage READMEs retain compact, plain-Markdown family blocks.
They use the pinned Ferramenta registry generator and require Node, pnpm, and
network access:

```sh
scripts/generate-readme-family.sh
scripts/generate-readme-family.sh --check
```

Update `generator_ref` in the generator script to adopt a new family revision.
For the root README, update `mdtheme.yaml` and regenerate separately. Commit
pins and outputs together. Never edit generated family text by hand.

`just readme` regenerates the native root README, the registry blocks, and the
crate company footers. `python3 scripts/mirror-readme-footer.py` copies the
outer Sebastian footer from the generated root README into the crate READMEs.
See [README and brand standard](docs/readme-standard.md).

## Code provenance

ferrolex is independently implemented and licensed `MIT OR Apache-2.0`.
Studying documented formats, observable behavior, concepts, and existing
implementations is permitted. Copying, file-by-file translation, mechanical
conversion, and side-by-side porting of incompatible implementations are not.

Spellbook must not be used as porting material. AI-assisted contributions are
reviewed for obvious structural closeness to known implementations and should
be prompted against ferrolex-owned behavior documentation rather than asking
for reproductions of other implementations. The pull-request template asks
contributors to attest to this review.

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

There are currently no license exceptions; keep this section as the review
point if a future dependency requires one.

See [ADR-0001](docs/adr/0001-code-provenance-policy.md) for the rationale and
the [GitHub delivery epics](https://github.com/sebastian-software/ferrolex/issues?q=is%3Aissue%20label%3Aepic)
for tracked requirements.

#
