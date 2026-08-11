# Performance

## Benchmark contract

Performance measurements in this repository are **local characterizations**,
not CI pass/fail gates or portable product claims. Run them from a clean,
release-profile checkout after the functional checks:

```sh
cargo test --workspace
cargo bench -p ferrolex-core
cargo bench -p ferrolex-compiler
```

Criterion retains raw estimates and confidence intervals beneath `target/` for
local comparison. To make a result reviewable, retain that output with the
reported commit, dirty-state, command, Rust toolchain (`rustc -Vv`), operating
system/architecture, CPU/power mode, and relevant background load. Do not
compare runs across machines or toolchains as if they were a single baseline.

Neither command performs filesystem I/O, process startup measurement, memory
measurement, nor a comparison with another spelling engine.

The compiled format is deliberately mmap-ready, not mmap-backed today. Its
loader reads a bounded byte slice and validates offsets without creating raw
pointers. `cargo bench -p ferrolex-compiler --no-run` is the CI compilation
gate for that benchmark; measurements remain an explicit local decision.

## Plain word-list lookup

`cargo bench -p ferrolex-core` measures present and absent exact lookups over
deterministically generated ASCII entries at 1,000, 10,000, and 100,000 words.
Dictionary construction is outside the measured closure; only `contains()` is
timed.

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
- Hunspell stems map to one or more lexeme records and then need affix and
  compound evaluation. Replacing their `BTreeMap` with an FST would require
  serializing terminal payload lists and revalidating that richer semantic
  path; it is not an exact-word drop-in.
- A hybrid can become worthwhile when profiling demonstrates that suggestion
  enumeration or stem prefiltering dominates. It needs a workload-specific
  benchmark before implementation.
