# Experimental Node.js and Python bindings

The `ferrolex-node` and `ferrolex-python` crates are Phase 8 evaluation
spikes. They are workspace-only and explicitly `publish = false`; no npm package,
Python wheel, or crates.io binding is published.

Both expose the same deliberately narrow API over newline-delimited UTF-8
plain-word-list text:

- `Checker(words)` creates an immutable checker;
- `check(word)` returns exact recognition; and
- `suggest(word)` returns deterministic, bounded spelling suggestions.

Neither prototype selects, downloads, or persists dictionaries, and neither
exposes source analysis. Those policy-bearing APIs need their own stabilized
Rust contracts before an external runtime can promise compatibility.

## Reproducible benchmark

The runtime checks build a native extension, load it in the target runtime, and
then compare 12,000 mixed recognized/missing queries against a native
`Set`/`set` baseline containing the same 4,097 words. They also assert that
`ferolex` suggests `ferrolex`.

```sh
bash scripts/bench-node-binding.sh
bash scripts/bench-python-binding.sh
```

Each command emits JSON with elapsed nanoseconds and the recognized-query
count. Timings are machine-local observations; equality of the count and the
successful extension import are the correctness gates. The checks run in CI.

## Decisions

### Node.js / napi-rs

**Maintained experimental workspace spike; deferred for npm publication.** The
prototype is useful for evaluating the CSpell-adjacent workflow and exposes no
hand-written unsafe adapter code. napi-rs generates the required Node-API
registration glue, so its isolated adapter crate allows generated unsafe code
while the ferrolex core crates continue to forbid it.

Release remains blocked on a supported prebuilt-binary matrix, npm package
ownership, TypeScript API policy, and a stable dictionary/source-analysis
surface. Current napi-rs requires Rust 1.88, which is the workspace MSRV.

### Python / PyO3 and maturin

**Maintained experimental workspace spike; deferred for wheel/PyPI
publication.** The prototype validates service-embedding ergonomics with the
current PyO3 API and includes a `pyproject.toml` for maturin builds.

Release remains blocked on an abi3/wheel support policy, interpreter and
platform matrix, package ownership, and a stable dictionary/source-analysis
surface. Python extension behavior is validated through a real import rather
than a Rust test binary, avoiding platform-specific embedded-interpreter
linker assumptions. The owner, source-only release channel, and promotion
criteria for both bindings are authoritative in
[ADR-0010](adr/0010-external-integration-support-tiers.md).
