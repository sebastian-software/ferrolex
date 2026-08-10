# Compiled dictionary format (version 1)

`ferrolex-compiler` writes the native exact-word format used by the initial
compiled-dictionary runtime. The format is deliberately small: metadata and
morphology are not silently encoded as implementation details. Future versions
will add those capabilities behind a new explicit format version and feature
bits.

All integer fields are unsigned little-endian. Sections are addressed by file
offsets, never pointers, and every section offset is a multiple of eight. The
compiler sorts UTF-8 words by byte order and removes duplicates, making the
same word set byte-identical regardless of input order or host platform.

## Header

The header is exactly 64 bytes.

| Offset | Width | Field |
| --- | ---: | --- |
| 0 | 8 | Magic: `FLEXDIC\\0` |
| 8 | 2 | Format version (`1`) |
| 10 | 2 | Header size (`64`) |
| 12 | 4 | Feature bits (zero in version 1) |
| 16 | 8 | FNV-1a 64 checksum |
| 24 | 8 | Number of unique words |
| 32 | 8 | Offset of the word-offset index |
| 40 | 8 | Offset of the UTF-8 data section |
| 48 | 8 | Byte length of the data section |
| 56 | 8 | Total file length |

The checksum is computed over the entire file with header bytes `16..24`
treated as zero. It cheaply detects accidental corruption; it is not a digital
signature and does not authenticate dictionary provenance.

## Sections

The index contains one 16-byte record per word, in lexical order:

| Relative offset | Width | Field |
| --- | ---: | --- |
| 0 | 8 | Start byte offset in the data section |
| 8 | 8 | Exclusive end byte offset in the data section |

The data section concatenates the UTF-8 words without terminators. The index
is exactly `word_count * 16` bytes and is followed by zero to seven padding
bytes before the aligned data section. Version 1's compiler produces no index
padding because the header and records are already eight-byte aligned; full
validation rejects any non-canonical padding.

## Loading and validation

`CompiledDictionary::load` performs fixed-header, section-boundary, and
checksum checks. It intentionally does not decode every word, preserving the
fast startup path required for a future mmap backing store. Lookup does a
bounds-checked binary search directly over word bytes and allocates nothing.

`CompiledDictionary::validate` is the opt-in paranoid check. It verifies every
index entry, data bounds, UTF-8 payload, non-empty word, and strict sort order.
Call it in CI, before distributing a file, or when accepting untrusted input.
Both paths treat every offset as untrusted and never create pointers from file
contents, following [ADR-0006](adr/0006-compiled-format-safety-and-layout.md).

## Compatibility

Version 1 readers reject a nonzero feature-bit field and any version other than
one. This fails closed when a future compiler emits semantics that an older
runtime cannot recognize.
