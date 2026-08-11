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

`cargo bench -p ferrolex-compiler` compares ferrolex's two exact-word
representations on the same deterministic 1,000 / 10,000 / 100,000-entry
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
