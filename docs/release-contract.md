# Pre-1.0 release contract (v0.2+)

This is the release and support contract for ferrolex's current pre-1.0
(`v0.2+`) releases. It turns the current native Rust library and CLI into an
explicit supported surface without promoting the diagnostic or
external-integration experiments.
applies to every release from `0.2.0` until a 1.0 contract replaces it.

## Versioning and MSRV

All public Rust workspace packages share one release version and changelog,
managed by Release Please. The MSRV is Rust **1.88**; CI builds, lints, and
tests the workspace with 1.88 and the current stable toolchain. The separate
nightly fuzz workspace is outside this MSRV contract.

ferrolex uses the following project and Cargo-compatible pre-1.0 convention:

- Patch releases (`0.y.z` to `0.y.z+1`) do not intentionally break the
  supported API, CLI, or artifact contract.
- Additive, backward-compatible changes may be released in a patch release.
- A breaking change to a supported surface, including an MSRV increase, needs
  a minor pre-1.0 release (`0.y.z` to `0.(y+1).0`) and release notes.
- Experimental and internal surfaces are excluded from the supported-surface
  promise, but intentional incompatible experimental changes also require the
  next minor release and release notes. Patches do not intentionally break an
  experimental surface. This is the project's Cargo-compatible release
  convention, not a claim about a generic SemVer pre-1.0 rule. Promotion still
  requires the evidence named in the relevant documentation; CI coverage or a
  source build alone is not promotion evidence.

## Support tiers

**Supported** means a documented, source-available public surface covered by
the release policy above. **Experimental** means maintained and tested where
documented, but it can change in a minor release and has no stable package,
ABI, protocol, or binary-distribution promise. **Internal** means a workspace
implementation or release helper, not an external dependency surface.

### Rust packages

| Package | Tier | Contract |
| --- | --- | --- |
| `ferrolex` | Supported, with an exception | Umbrella crate; re-exports the supported core dictionary API. Its `ffi` feature is experimental because it enables the experimental `ferrolex-ffi/c-abi` surface. |
| `ferrolex-core` | Supported | Dictionary interfaces, word lists, checkers, normalization, and user overlays. |
| `ferrolex-text` | Supported | Plain-text tokenization and checking. |
| `ferrolex-code` | Supported | Generic source-code analysis and its documented configuration contract. |
| `ferrolex-hunspell` | Supported, with an exception | Documented import and runtime-cache APIs. The lookup-explanation types and `explain` path remain experimental as stated in [Explanations](explanations.md). |
| `ferrolex-suggest` | Supported | Bounded, deterministic suggestions. |
| `ferrolex-compiler` | Supported | The documented compiled-dictionary API and format reader/writer. Its neutral IR is an implementation model, not a wire-format promise. |
| `ferrolex-dictionaries` | Supported | Explicit, verified source acquisition; it does not bundle or redistribute dictionary data. |
| `ferrolex-cli` | Internal | Packaging crate for the supported `ferrolex` executable; it is not a library integration surface. |
| `ferrolex-ffi` | Experimental | Workspace-only C ABI spike; `publish = false`. See [C ABI](ffi.md). |
| `ferrolex-node` | Experimental | Workspace-only Node.js binding spike; `publish = false`. See [Bindings](bindings.md). |
| `ferrolex-python` | Experimental | Workspace-only Python binding spike; `publish = false`. See [Bindings](bindings.md). |
| `ferrolex-lsp` | Experimental | Source-built stdio server; `publish = false` and no release artifact exists yet. See [LSP](lsp.md). |

The supported Rust API is the documented public API of the supported packages,
except where a package or its API documentation explicitly labels an item
experimental. Non-public items, tests, benchmarks, build scripts, generated
headers, and the neutral IR's serialization are internal. Rust API
documentation is generated from source during CI; there is no claim that an
unreleased package has already been published to docs.rs.

### Executables and CLI commands

| Executable or command | Tier | Contract |
| --- | --- | --- |
| `ferrolex check`, `suggest`, `analyze`, `compile`, `inspect`, and `validate` | Supported | The documented command syntax and behavior in the README and format/import documentation. |
| `ferrolex dictionary list`, `fetch`, `install`, and `add-word` | Supported | Explicit catalog and local-cache operations under [Dictionary fetching](dictionary-fetching.md); normal commands never download implicitly. |
| `ferrolex explain` | Experimental | Diagnostic output follows [Explanations](explanations.md), not a stable serialization or API contract. |
| `ferrolex --help` / `-h` | Supported | Help for the supported command surface. |
| `ferrolex-lsp` | Experimental | Maintained source-built server with no released artifact or protocol-compatibility promise. |
| `generate-header` | Internal | Header-maintenance helper for the experimental C ABI, not an end-user executable. |

No current workflow publishes a downloadable CLI, LSP, binding, or C-ABI
binary. The supported CLI is therefore a source-built Cargo executable; the
separate external-distribution plans remain governed by
[ADR-0010](adr/0010-external-integration-support-tiers.md).

## Dictionary artifact compatibility

Artifact compatibility is controlled by the artifact's own format and
semantics versions, not merely by the Rust package version.

- `FLEXDIC` version 1 is the supported native compiled-dictionary format. A
  reader accepts only its known version and feature bits, and must reject
  unknown versions or features rather than silently reinterpret them. For the
  same source, compiler version, and options, compilation is byte-identical.
- `FLXHSP` runtime caches carry format, semantics, provenance, and checksum
  information. A source-associated cache is disposable and must be rebuilt
  when its loader rejects it. A standalone artifact is accepted only when its
  recorded versions and integrity checks match the reader.
- A new artifact layout or semantics requires an explicit version or feature
  gate and fail-closed behavior in older readers. There is no promise that an
  older reader accepts a newer artifact.

See [Compiled dictionary format](binary-format.md) and [Hunspell runtime
cache](hunspell-runtime-cache.md) for the exact current formats and bounds.

## Release gate

A `v0.2+` release candidate requires all configured CI jobs to be green,
including the Rust 1.88/stable test matrix, compatibility fixtures, fuzz
smoke, binding spikes, and VS Code extension checks. The dependency-policy job
runs `cargo deny check` and verifies both distribution licenses; its advisory,
license, source, banned-dependency, and yanked-package policy is maintained in
[`deny.toml`](../deny.toml). The PR-title check and Release Please workflow
continue to enforce the release process.

Documentation is part of the gate: the README and linked contract documents
must describe every supported and experimental surface, and CI must generate
the public package docs without dependencies. The internal `ferrolex-cli`
binary is deliberately excluded from that docs command because its binary name
collides with the public `ferrolex` library's rustdoc output; the checker uses
an isolated target directory and verifies the public umbrella page instead.

Before creating or tagging a `v0.2+` release candidate, run the following
source-based reproducibility verification from a clean checkout with Rust 1.88
installed:

```sh
bash scripts/verify-release.sh
```

It builds the locked workspace in release mode, compiles the same plain-word
list twice, compares the resulting `FLEXDIC` artifacts byte-for-byte, and
validates the artifact. It intentionally does not use `cargo package` as a
pre-publication gate: until the coordinated workspace packages exist on
crates.io, Cargo cannot resolve the umbrella crate's published dependency
graph. Packaging verification belongs to the staged publication procedure once
those package versions are available.

## Documentation map

- [README](../README.md): project status and CLI entry point.
- [Compatibility](compatibility.md), [Hunspell format](hunspell-format.md),
  [source analysis](source-code-analysis.md), and [suggestions](suggestions.md):
  supported behavior boundaries.
- [Native integrations](integrations.md), [C ABI](ffi.md), [bindings](bindings.md),
  [LSP](lsp.md), and [VS Code](vscode.md): experimental external surfaces.
- [Architecture decision records](adr/README.md): durable rationale, including
  [ADR-0010](adr/0010-external-integration-support-tiers.md).
