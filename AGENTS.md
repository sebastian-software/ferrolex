# Repository guidance

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
  deterministic CI-parity validation. `just fuzz-smoke` is an explicit,
  nightly-only opt-in.
- The supported MSRV is declared in the workspace `Cargo.toml`; the pinned
  `rust-toolchain.toml` and helper scripts read or verify that policy.
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
