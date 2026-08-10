# ADR-0006: Compiled dictionary format — tolerant mmap, little-endian, byte-identical

- Status: accepted
- Date: 2026-08-10
- Last updated: 2026-08-10
- Deciders: Sebastian Werner

## Context

Memory-mapped compiled dictionaries are the project's primary startup
optimization (RFC §13), but a mapped file can be modified by other processes
while in use and must be treated as untrusted bytes at all times (RFC §39).
Full structural validation at load time would recover safety but give back a
large part of the mmap startup advantage. The format also needs fixed
endianness, alignment, and reproducibility rules for determinism (RFC §42).

## Decision

The compiled format is designed to tolerate arbitrary bytes without undefined
behavior:

- Every access into the mapped region is bounds-checked; indices and offsets
  are validated at use. Corrupted or hostile input may at worst produce wrong
  spell-check results — never memory unsafety. (The `fst` crate demonstrates
  this model in production.)
- Loading performs only a fast header and checksum check.
- Full structural validation is an opt-in: `ferrolex validate`, CI usage, and
  a paranoid loading mode.

Format layout:

- Fixed little-endian encoding (all relevant target platforms are LE;
  big-endian hosts may byte-swap on load or skip the mmap fast path).
- Sections aligned to 8 bytes, addressed by offsets rather than pointers.
- Compilation output is byte-identical across platforms for the same input,
  compiler version, and options — no hash-map iteration order, no
  platform-dependent formatting, stable sorts only.

## Considered options

### Full validation at load (rejected)

Simpler safety reasoning, but costs startup time proportional to dictionary
size — exactly what the compiled format exists to avoid.

### In-memory copy as default, mmap opt-in (rejected)

Most conservative, but gives up the startup advantage almost entirely.

### Tolerant format with opt-in validation (chosen)

Keeps the startup advantage, contains the risk to wrong-results-not-UB, and
gives strict environments a paranoid mode.

## Consequences

- The loader and lookup code carry the bounds-checking discipline; fuzzing
  the compiled-dictionary loader (RFC §37) is the enforcement mechanism.
- The format specification must document the header, checksum, alignment, and
  endianness rules from the first version.

## Validation and review triggers

- Reopen if fuzzing or audits show the bounds-checked model is too easy to
  get wrong in practice, or if a big-endian target becomes relevant.

## References

- [rfc.md](../../rfc.md) §12, §13, §37, §39, §42
