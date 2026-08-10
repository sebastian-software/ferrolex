# Robustness testing

Dictionary files and suggestion input are treated as untrusted. The regular
test suite therefore includes deterministic adversarial regression corpora;
they require no separate fuzzer installation and run on the project's Rust
1.81 MSRV in CI.

The corpus tests cover these boundaries:

- Hunspell import: malformed headers, oversized numeric fields, incomplete
  affix groups, invalid conditions, empty or excessive flag sections, Unicode,
  and dictionary entries without stems. Both parsing and representative lookup
  run inside the no-panic assertion.
- Hunspell runtime caches: format and semantics versions, source-provenance
  mismatches, checksums, counts, truncations, trailing payload bytes, and a
  deterministic mutation corpus. A cache is constructed only after its full
  structure is validated and never yields a partial dictionary.
- Compiled dictionaries: every truncation and single-byte mutation of a valid
  artifact, followed by a fixed seeded byte corpus. Each accepted artifact is
  then fully validated and queried.
- Suggestions: empty, Unicode, oversized case-expanding input, and each zero
  work-budget configuration. Results must stay within their configured output
  budget and every case must complete without a panic.

These tests are a deterministic regression corpus, rather than a claim of
coverage-guided fuzzing. They protect the current parser, loader, compound,
and suggestion limits on every ordinary CI run. A future dedicated
`cargo-fuzz` campaign can reuse these cases as seed inputs without changing
the public crates or their MSRV.

Run the focused suite with:

```sh
cargo +1.81 test -p ferrolex-hunspell -p ferrolex-compiler -p ferrolex-suggest
```

Run the complete repository gate with the commands in CI:

```sh
cargo +1.81 fmt --all -- --check
cargo +1.81 clippy --workspace --all-targets -- -D warnings
cargo +1.81 test --workspace
```
