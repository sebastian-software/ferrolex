# Robustness testing

Dictionary files and suggestion input are treated as untrusted. The regular
test suite therefore includes deterministic adversarial regression corpora;
they require no separate fuzzer installation and run on the project's Rust
1.88 MSRV in CI.

Property tests use `proptest` with its default persisted regression seeds. They
exercise normalization idempotence, deterministic compiled-dictionary output,
bounded UTF-8 suggestion output, Hunspell cache recognition round-trips, and
cache-loader behavior on arbitrary byte input. The strategies are deliberately
bounded so the ordinary CI test lane remains predictable.

The corpus tests cover these boundaries:

- Hunspell import: malformed headers, oversized numeric fields, incomplete
  affix groups, invalid conditions, empty or excessive flag sections, Unicode,
  and dictionary entries without stems. Both parsing and representative lookup
  run inside the no-panic assertion.
- Hunspell runtime caches: format and semantics versions, source-provenance
  mismatches, checksums, counts, truncations, trailing payload bytes, readable
  header metadata, and a deterministic mutation corpus. A cache is constructed
  only after its full structure is validated and never yields a partial
  dictionary.
- Compiled dictionaries: every truncation and single-byte mutation of a valid
  artifact, followed by a fixed seeded byte corpus. Each accepted artifact is
  then fully validated and queried.
- Suggestions: empty, Unicode, oversized case-expanding input, and each zero
  work-budget configuration. Results must stay within their configured output
  budget and every case must complete without a panic.

These tests are a deterministic regression corpus. They protect the current
parser, loader, compound, and suggestion limits on every ordinary CI run; the
same representative cases seed the dedicated coverage-guided fuzzing corpus
under `fuzz/corpus/`.

The regular CI license job checks that both root license texts are present and
non-empty. Third-party dictionary sources are not bundled; their manifest and
fixture entries keep license evidence separately from engine code.

Run the focused suite with:

```sh
cargo +1.88 test -p ferrolex-hunspell -p ferrolex-compiler -p ferrolex-suggest
```

## Coverage-guided fuzzing

The nightly-only `fuzz/` workspace deliberately sits outside the shipped
workspace and its Rust 1.88 MSRV contract. Its targets cover raw Hunspell
import in every supported byte encoding, the FLXHSP runtime-cache loader, the
FLEXDIC loader, suggestion queries, and compound evaluation. Their initial
corpus consists of the deterministic robustness cases above; minimize and add
any crash or sanitizer finding to the appropriate target corpus before fixing
it and file the finding as a GitHub issue.

Run a short local smoke pass with:

```sh
cargo +nightly install cargo-fuzz --locked
for target in hunspell_import runtime_cache_loader compiled_loader suggestion_input compound_evaluation; do
  cargo +nightly fuzz run "$target" -- -runs=256
done
```

For an investigation, run one target without `-runs` and stop it manually. CI
runs the bounded smoke pass only; it is evidence that every fuzz boundary
builds and executes, not a substitute for a sustained local campaign.

Run the complete repository gate with the commands in CI:

```sh
cargo +1.88 fmt --all -- --check
cargo +1.88 clippy --workspace --all-targets -- -D warnings
cargo +1.88 test --workspace
```
