## Summary

<!-- What changed, and why? Keep the scope focused on one issue. -->

## Changes

<!-- The notable changes, one bullet each. Call out user-visible or breaking behavior. -->

## Validation

- [ ] I ran the focused tests for the changed area.
- [ ] I ran `cargo fmt --all -- --check`.
- [ ] I ran `cargo clippy --workspace --all-targets -- -D warnings` when Rust code changed.
- [ ] I ran `cargo test --workspace` when Rust code or behavior changed.
- [ ] I ran `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` when public Rust docs changed.
- [ ] I ran `just gate` when dependencies, release metadata, or the version contract changed.

## Documentation and release impact

- [ ] User-facing behavior, public API, or compatibility documentation is updated when needed.
- [ ] This change does not require a release-note or version-contract update, or I have included it.

## Provenance

- [ ] I confirm that this change is independently implemented and does not copy, mechanically translate, or side-by-side port an incompatible implementation.
- [ ] If AI assistance was used, I reviewed the result against ferrolex-owned behavior documentation and checked it for structural closeness to known implementations.
- [ ] I did not add third-party dictionary content or files with unclear redistribution rights.

## Issue

<!-- Closes #123, Refs #123, or a short note on why no issue exists. -->
