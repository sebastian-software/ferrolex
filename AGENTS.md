# Repository guidance

Read this file together with [CONTRIBUTING.md](CONTRIBUTING.md) before
changing code or durable repository artifacts.

## Language and commits

- All durable repository artifacts use US English
  ([ADR-0003](docs/adr/0003-project-language-us-english.md)).
- Commit messages follow Conventional Commits and Release Please derives
  versions and changelogs from them; CI validates the pull-request title as
  well ([ADR-0004](docs/adr/0004-conventional-commits-and-release-please.md)).

## Contribution boundaries

- Keep ferrolex independently implemented. Do not use Spellbook as porting
  material; review AI-assisted changes for structural closeness to known
  implementations. See [CONTRIBUTING.md](CONTRIBUTING.md) and
  [ADR-0001](docs/adr/0001-code-provenance-policy.md).
- Treat the public Rust engine, CLI, Hunspell compatibility, suggestions, and
  the Node.js package as the product boundary. Keep the C ABI, Python, LSP, and
  VS Code work within their documented prototype tiers.

## Validation

- Use `just quick` for the fast local product-crate loop and `just gate` for
  deterministic CI-parity validation. `just coverage` reproduces the CI
  line-coverage gate and needs cargo-llvm-cov; `just fuzz-smoke` is an
  explicit, nightly-only opt-in.
- The supported MSRV is declared in the workspace `Cargo.toml`; the pinned
  `rust-toolchain.toml` and helper scripts read or verify that policy.
- The Ferramenta family block in the README surfaces is generated from the
  registry in [ferramenta](https://github.com/sebastian-software/ferramenta);
  never edit it between its markers. Run `just readme` to re-render it (needs
  pnpm and network access); CI checks it with
  `scripts/generate-readme-family.sh --check`. See
  [CONTRIBUTING.md](CONTRIBUTING.md#readme-family-block).
- Real-world Hunspell fixtures are opt-in and must never be committed. Use
  `FERROLEX_COMPAT_FIXTURES` with the checked-in manifest; optionally select
  `FERROLEX_COMPAT_FIXTURE_SET=required|scorecard|all`.
- Oracle and scorecard runs may additionally use `FERROLEX_COMPAT_ORACLE`,
  `FERROLEX_COMPAT_SCORECARD`, and
  `FERROLEX_COMPAT_SCORECARD_BASELINE`. See
  [compatibility-fixtures.md](docs/compatibility-fixtures.md).

## Decision records

The authoritative ADR index is [docs/adr/README.md](docs/adr/README.md).
Update the relevant living ADR when a change alters a documented architectural
or product-boundary decision.

---

<!-- sebastian-software-consumer-agents:start -->

# Standards-managed repo guardrails

- Do not hand-edit managed files or standards-owned marker sections.
- If `standards check` reports drift, run `standards apply` or update standards.
- The repository's own gate may omit `standards check`; CI can still fail on it.

Node repositories:

- Fix or format every file reported by `oxfmt` whenever practical.
- For generated files, prefer formatting in the generator step.
- If formatting is not viable, use repo-local `.prettierignore`.
- Never add repo-specific ignores to managed `.oxfmtrc.json`.

Rust repositories:

- Keep `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings` green.
- Lint levels belong in `[workspace.lints]`, never in managed `rustfmt.toml`.
- `rust-version` in `Cargo.toml` is the only MSRV; every other mention is a
  derived copy.
- Record a cargo-deny finding as a narrow, commented exception in `deny.toml` —
  never by widening the org allow-list.

<!-- sebastian-software-consumer-agents:end -->
