# Performance

## Benchmark contract

Performance measurements in this repository are **local characterizations**,
not CI pass/fail gates or portable product claims. Run them from a clean,
release-profile checkout after the functional checks:

```sh
cargo test --workspace
cargo bench -p ferrolex-core
cargo bench -p ferrolex-compiler
cargo bench -p ferrolex-hunspell
cargo bench -p ferrolex-suggest
```

Criterion retains raw estimates and confidence intervals beneath `target/` for
local comparison. To make a result reviewable, retain that output with the
reported commit, dirty-state, command, Rust toolchain (`rustc -Vv`), operating
system/architecture, CPU/power mode, and relevant background load. Do not
compare runs across machines or toolchains as if they were a single baseline.

Neither command performs filesystem I/O, process startup measurement, memory
measurement, nor a comparison with another spelling engine.

## Hunspell morphology and developer workloads

`cargo bench -p ferrolex-hunspell` measures one strict, in-source synthetic
Hunspell pair. Its lookup lanes are `hit`, `miss`, `affixed`, `compound`, and
`mixed-case`; the fixture asserts each lane's recognition result before timing.
It also measures the dominant startup and runtime paths against a deterministic
100,000-entry Hunspell source: strict `.aff`/`.dic` import, validated runtime
cache loading, source and cache parity for hit / affixed / miss lookups, and an
affix-derived suggestion. The import source and runtime artifact are prepared
outside the timed closures; import and cache-load measurements include parsing,
validation, allocation, construction, and destruction, but no filesystem I/O.

The same benchmark checks four pinned synthetic repository-shaped workloads:
large Markdown, TypeScript, Rust, and mixed documentation/code. Corpus text,
dictionary construction, and analyzer construction are outside the timed
closures. Every lookup and suggestion lane asserts its intended result before
timing. The results characterize the named in-memory paths, not process startup
or a claim about any external repository.

There is intentionally no `ferrolex benchmark` CLI command. Criterion benches
are the supported developer measurement path because they make the exact
workload, toolchain, and statistical output explicit without expanding the
runtime CLI surface.

### External Hunspell comparison

To investigate high-volume lookup, install Hunspell outside this repository,
use a digest-verified fixture from the compatibility suite,
and compare only the same preloaded word sequence on one quiet machine. Record
the Ferrolex and Hunspell commands, dictionary digest, query mix, toolchain,
OS/CPU/power mode, and raw Criterion output. Treat the external executable as
a development-only black-box oracle: do not add it as a production dependency,
do not report cross-machine ratios as a product guarantee, and investigate any
recognition mismatch before interpreting timing results.

### Real-world startup, memory, and miss-path characterization

The following audit measurements answer deployment questions that the
in-memory Criterion lanes intentionally do not answer. They were recorded on
an Apple M1 Pro in a release build using a counting allocator, `ps`/`time -l`
RSS sampling, and hyperfine against the pinned 258,219-entry de_DE scale. They
are local characterizations, not CI gates or cross-machine product promises.

| Path or workload | Observed result | Scope |
| --- | ---: | --- |
| Runtime-cache load | ~200 ms | In-process load at de_DE scale; filesystem and process startup are not separated by this number. |
| Cold single-word CLI check | ~0.22 s | One process invocation including startup and local dictionary/cache setup. |
| Whole-dictionary heap | ~89 MiB | Full imported representation, not the empty morphology slice alone. |
| Whole-dictionary process RSS | ~119 MB | Same scale, including allocator and process/runtime overhead. |
| Hit-heavy text checking | ~24 MB/s | Realistic tokenization/checking path with mostly recognized words. |
| Mixed real Markdown checking | ~0.03 MB/s | Miss-dominated path; the effective ceiling is not the exact-hit lookup lane. |

Miss cost varies with the spelling and casing path: the audit measured roughly
79 µs for short lowercase misses, 103–110 µs for long lowercase misses,
173–185 µs for capitalized misses, and 210–222 µs for uppercase misses. The
capitalized and uppercase paths repeat case analysis, while affix-shaped
misses spend most of their time collecting and checking derived candidates.
These figures are tracked with the [round-2 miss-path issue](https://github.com/sebastian-software/ferrolex/issues/212)
and should be rerun after that implementation changes or when the memory work
in [#111](https://github.com/sebastian-software/ferrolex/issues/111) lands.

The memory figures are intentionally also repeated in
[Hunspell runtime cache](hunspell-runtime-cache.md), where the 3.94 MiB empty
morphology slice and 0.99 MiB empty-field count are scoped as component costs
rather than presented as the full dictionary footprint.

## Suggestions

`cargo bench -p ferrolex-suggest` measures the reused-buffer
`Suggester::suggest_into` path against a deterministic, exactly 100,000-word
corpus.
The five lanes are `single-edit`, `transposition`, `long-word`,
`compound-typo`, and `no-useful-suggestion`. Their target spellings are added
to the same synthetic corpus so every lane has a stable intended outcome (or,
for the last lane, intentionally does not). Before Criterion times a lane, the
benchmark requires a complete, non-budget-truncated result and verifies the
expected suggestion or stable empty output.

Corpus construction, sorting, `Suggester` construction, and initial buffer
allocation happen outside Criterion's timed closure. Each measurement retains
one output vector and one `SuggestScratch`, so it characterizes steady-state
candidate traversal, ranking, and the amortized allocation behavior promised by
the buffer API. Compare it with the default Criterion settings and the same
machine/toolchain when deciding whether an allocation change is worthwhile.
CI runs `cargo bench -p ferrolex-suggest --no-run`; it checks that the benchmark
continues to compile without turning local measurements into a timing gate.

The compiled format is deliberately mmap-ready, not mmap-backed today. Its
loader reads a bounded byte slice and validates offsets without creating raw
pointers. `cargo bench -p ferrolex-compiler --no-run` is the CI compilation
gate for that benchmark; measurements remain an explicit local decision.

## Plain word-list lookup

`cargo bench -p ferrolex-core` measures present and absent exact lookups over
deterministically generated ASCII entries at 1,000, 10,000, and 100,000 words.
Dictionary construction is outside the measured closure; only `contains()` is
timed.

`WordList` stores unique entries in one contiguous UTF-8 arena and keeps one
32-bit start offset per entry. This avoids one heap allocation per word while
preserving deterministic lexical iteration and allocation-free exact lookup.
For repeated editor or CLI checks, compile the source once and use
`ferrolex check --compiled dictionary.flexh`; the compiled artifact is the
intended startup path when reparsing a plain-text list for every invocation
would be unnecessary overhead.

## Plain text versus compiled dictionary

`cargo bench -p ferrolex-compiler` compares ferrolex's exact-word
representations on the same deterministic 1,000 / 10,000 / 100,000 /
250,000-entry
synthetic UTF-8 corpus. The corpus cycles among ASCII (`alpha…`), German
multi-byte (`straße…`), and Japanese multi-byte (`東京…`) words. It is generated
in source and has no linguistic-coverage claim.

Before a timing lane runs, the benchmark asserts that `WordList` and
`CompiledDictionary` agree for one present and one absent query; the loading
lane additionally checks every generated entry and count. The lookup lanes time
only present or absent `Dictionary::contains()` calls after each representation
has been constructed. The loading lanes time construction from an owned
in-memory artifact copy: plain UTF-8 text into `WordList`, or compiled bytes
through `CompiledDictionary::load`. Thus, they characterize format-specific
parsing, validation, allocation, and copying; they do not equate the formats'
on-disk sizes or claim a universal startup result.

Fixture generation, compilation of the binary artifact, semantic parity checks,
and all file I/O remain outside Criterion's timed closures. Criterion chooses
warmup and samples according to its configured defaults. Repeat a surprising
result with the same command, corpus parameters, toolchain, and machine state
before using it for an engineering decision.

## Lookup-structure evaluation (2026-08-11)

Issue #38 evaluates the sorted exact-word tables against a minimal finite-state
set (the `fst` crate) at the 250,000-word lane. That lane is deliberately close
to the 258,219-entry pinned German fixture and is large enough to expose the
different lookup shapes while keeping the input reproducible. The FST is a
benchmark-only dev dependency; it is not part of the shipped runtime.

Command:

```sh
cargo bench -p ferrolex-compiler --bench dictionary -- \
  'exact lookup parity/(word-list|compiled|fst)/(present|absent)/250000' \
  --sample-size 10 --warm-up-time 1 --measurement-time 1
```

Recorded on a clean `main` checkout before the #38 commit, Darwin arm64,
`rustc 1.95.0-nightly (842bd5be2 2026-01-29)`. The intentionally short
Criterion configuration is a comparative characterization, not a portable
claim; repeat it with the default Criterion settings before changing the
decision.

| Lookup, 250,000 generated UTF-8 words | Present query | Absent query | Result |
| --- | ---: | ---: | --- |
| `WordList` sorted table | 144.94 ns | 109.25 ns | Baseline plain representation. |
| `CompiledDictionary` sorted offset table | 79.51 ns | 76.65 ns | Shipped exact artifact; fastest accepted-word lane. |
| Minimal FST (`fst::Set`) | 108.95 ns | 6.82 ns | Candidate: very fast rejection, but slower accepted-word lane and a different traversal model. |

The table deliberately keeps neither a synthetic result nor a local CPU as a
product-performance promise.

### Decision

Keep sorted tables for now. The current native format gives allocation-free
binary search, deterministic byte-identical output, direct lexical candidate
iteration, and a small fully owned loader. A minimal FST is a promising future
option only if repeated full-scale measurements show a clear combined win for
the dominant workload (including artifact size, startup, present *and* absent
lookup, and candidate streaming), not merely one lookup direction.

Other candidates are not adopted:

- Compressed/radix tries optimize prefix navigation, but exact lookup has no
  prefix-query requirement and a pointer-rich implementation would conflict
  with the current compact, bounds-checked artifact layout.
- Perfect hashing favors static membership only; it does not preserve the
  deterministic lexicographic traversal required for suggestion candidates,
  and its generated tables would need a new reproducibility contract.
- A hash sidecar could retain the sorted table for candidate traversal while
  accelerating exact rejection. It would add a second serialized structure,
  hash computation and lookup work, memory overhead, and another deterministic
  format/versioning contract. More importantly, it would not remove the
  affix-derived candidate search that dominates real Hunspell misses. The
  option is therefore not adopted without a benchmark that measures the full
  de_DE-scale workload, including artifact size, cache load, hit/miss mixes,
  suggestions, and resident memory.
- Hunspell stems map to one or more lexeme records and then need affix and
  compound evaluation. Replacing their `BTreeMap` with an FST would require
  serializing terminal payload lists and revalidating that richer semantic
  path; it is not an exact-word drop-in.
- A hybrid can become worthwhile when profiling demonstrates that suggestion
  enumeration or stem prefiltering dominates. It needs a workload-specific
  benchmark before implementation.
