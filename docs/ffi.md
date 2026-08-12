# Experimental C ABI

`ferrolex-ffi` is a Phase 8 design spike. It is not published and it does not
make a stability promise yet. The default Rust workspace build does not compile
or expose this ABI; it is enabled explicitly through `c-abi` (or the umbrella
crate's `ffi` feature).

## Scope of the spike

The ABI deliberately covers only a small, self-contained path:

- create an immutable checker from UTF-8, newline-delimited plain-word-list
  text;
- check one UTF-8 word; and
- request bounded, deterministic suggestions using a caller-owned byte buffer.

It does not load Hunspell files, fetch dictionaries, persist user words, choose
locales, or expose source analysis. Those policies need their own stable Rust
contracts before they can safely cross an ABI boundary.

## Ownership and threading

`FerrolexChecker` is opaque. A successful constructor call gives its caller one
handle; call `ferrolex_checker_free` exactly once when no other call is using
that handle. Lookup and suggestion operations only read the immutable checker
and may run concurrently. Creating, freeing, or reusing an invalid, released,
or foreign handle is outside the C API contract.

All textual input is a `(const uint8_t *, size_t)` UTF-8 span. A null pointer
is valid only with length zero. No input needs a trailing NUL byte. Invalid
UTF-8 produces `FerrolexStatus_InvalidUtf8`.

## Suggestions and errors

`ferrolex_checker_suggest` writes NUL-separated UTF-8 spellings with no
trailing NUL. First call it with `buffer = NULL` and `buffer_length = 0`; it
returns `BufferTooSmall` when suggestions exist while also writing the exact
byte count and suggestion count. Allocate that many bytes and call again. An
empty result needs no buffer and returns `Ok`.

Every fallible operation returns `FerrolexStatus`; Rust panics are caught and
reported as `Panic`, never unwound across the C boundary. The only exception is
the conventional C ownership precondition: an invalid opaque handle cannot be
made safe by a raw-pointer API.

## Header and build

The checked-in [header](../crates/ferrolex-ffi/include/ferrolex.h) is generated
from the exported Rust declarations by `cbindgen`. Regenerate it after an ABI
change:

```sh
cargo run -p ferrolex-ffi --features c-abi --bin generate-header
```

Normal `c-abi` builds regenerate the same header into Cargo's `OUT_DIR` and
fail if it differs from the checked-in file. This keeps the source-distributed
header synchronized with the symbols that Rust compiles.

## Decision record

The C ABI is a **go for a maintained experimental spike**, not yet a go for a
stable compatibility promise or crates.io publication. Promote it only after
the Rust check/suggest/source-analysis APIs and binary distribution policy have
stabilized, and after a real C consumer validates the ownership and packaging
model.
