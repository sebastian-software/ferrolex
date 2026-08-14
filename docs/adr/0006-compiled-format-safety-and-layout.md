# ADR-0006: Compiled dictionary format — owned loading, little-endian, byte-identical

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-11
- Deciders: Sebastian Werner

## Context

The initial compiled format was described as mmap-ready, but the workspace
forbids unsafe code and the current loader owns a `Vec<u8>`. A whole-file
checksum would also fault in every mapped page before lookup, eliminating the
claimed mmap startup path. The format still needs fixed endianness, alignment,
reproducibility, and fail-closed compatibility rules.

## Decision

Compiled dictionary artifacts stay owned-memory backed. The format is designed
to tolerate arbitrary bytes without undefined behavior:

- Every access into the mapped region is bounds-checked; indices and offsets
  are validated at use. Corrupted or hostile input may at worst produce wrong
  spell-check results — never memory unsafety. (The `fst` crate demonstrates
  this model in production.)
- Loading performs only a fast header and checksum check over the owned bytes.
- Full structural validation is an opt-in: `ferrolex validate`, CI usage, and
  a paranoid loading mode.

Format layout:

- Fixed little-endian encoding (big-endian hosts may byte-swap at the owned
  loader boundary).
- Sections aligned to 8 bytes, addressed by offsets rather than pointers.
- Compilation output is byte-identical across platforms for the same input,
  compiler version, and options — no hash-map iteration order, no
  platform-dependent formatting, stable sorts only.

## Considered options

### Full validation at load (rejected)

Simpler safety reasoning, but costs startup time proportional to dictionary
size — exactly what the compiled format exists to avoid.

### Owned-memory loader (chosen)

Matches the safe-Rust workspace boundary and still permits a substantial win
over textual parsing without introducing a platform-specific unsafe module.

### Mmap now (deferred)

Requires a separate audited unsafe boundary, a page-friendly integrity design
(header or per-section checksums), fuzz coverage, and reproducible startup
measurements. It is not a loader convenience change.

## Consequences

- The loader and lookup code carry the bounds-checking discipline; fuzzing the
  compiled-dictionary loader is the enforcement mechanism.
- The format specification must document the header, checksum, alignment, and
  endianness rules from the first version.
- New layouts must reject overlapping sections during fast loading.

## Validation and review triggers

- Reopen if fuzzing or audits show the bounds-checked model is too easy to
  get wrong in practice, a big-endian target becomes relevant, or measured
  startup data justifies an audited mmap design.

## References

- [Compiled dictionary format](../binary-format.md)
- [Performance measurement contract](../performance.md)
