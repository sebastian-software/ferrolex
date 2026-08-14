# Node.js and deferred binding prototypes

`ferrolex-node` is the selected first direct non-Rust integration. It remains
unpublished while its npm package, TypeScript API, supported binary targets,
managed dictionary workflow, and clean-install verification are defined.

`ferrolex-python` is an evaluation prototype outside the current product scope.
There is no PyPI package, wheel matrix, or compatibility commitment.

Both expose the same deliberately narrow API over newline-delimited UTF-8
plain-word-list text:

- `Checker(words)` creates an immutable checker;
- `check(word)` returns exact recognition; and
- `suggest(word)` returns deterministic, bounded spelling suggestions.

The current narrow APIs still accept newline-delimited word lists. Aligning the
Node.js binding with managed Hunspell dictionaries and suggestions is the next
integration step. Python does not drive that design.

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
successful extension import are the correctness gates. Both checks still run
while the workspace is being simplified, but only Node.js belongs to the
current release direction.

## Decisions

### Node.js / napi-rs

**Selected pre-1.0 integration; deferred for npm publication.** The adapter
exposes no
hand-written unsafe adapter code. napi-rs generates the required Node-API
registration glue, so its isolated adapter crate allows generated unsafe code
while the ferrolex core crates continue to forbid it.

Release remains blocked on a supported prebuilt-binary matrix, npm package
ownership, TypeScript API policy, and the managed dictionary workflow. Current
napi-rs requires Rust 1.88, which is the workspace MSRV.

### Python / PyO3 and maturin

**Evaluation prototype outside the current product scope.** The checked-in code
validates service-embedding ergonomics with the current PyO3 API and includes a
`pyproject.toml` for maturin builds, but there is no wheel or PyPI plan.

The prototype does not drive the Node.js API, platform matrix, or release gate.
See [ADR-0010](adr/0010-external-integration-support-tiers.md).
