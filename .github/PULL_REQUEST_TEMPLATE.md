## Summary

<!-- What changed, and why? Keep the scope focused on one issue. -->

## Validation

- [ ] I ran the focused tests for the changed area.
- [ ] I ran `cargo +1.88 fmt --all -- --check`.
- [ ] I ran `cargo +1.88 clippy --workspace --all-targets -- -D warnings` when Rust code changed.
- [ ] I ran `cargo +1.88 test --workspace` when Rust code or behavior changed.
- [ ] I ran `RUSTDOCFLAGS="-D warnings" cargo +1.88 doc --workspace --no-deps` when public Rust docs changed.

## Documentation and release impact

- [ ] User-facing behavior, public API, or compatibility documentation is updated when needed.
- [ ] This change does not require a release-note or version-contract update, or I have included it.

## Provenance

- [ ] I confirm that this change is independently implemented and does not copy, mechanically translate, or side-by-side port an incompatible implementation.
- [ ] If AI assistance was used, I reviewed the result against ferrolex-owned behavior documentation and checked it for structural closeness to known implementations.
- [ ] I did not add third-party dictionary content or files with unclear redistribution rights.
