# Hunspell runtime cache

`ferrolex dictionary install` retains the exact verified LibreOffice source
files and derives a local runtime cache beside the affix file:

```text
<cache>/<locale>/<affix-stem>.ferrolex-hunspell-v1.flexh
```

The cache is an implementation artifact. The original `.aff` and `.dic` bytes
remain the authoritative, license-bearing inputs.

## Validity contract

The current format uses the `FLXHSP\0\0` marker, little-endian fields, a payload
SHA-256 checksum, and SHA-256 fingerprints of both unmodified source files. It
also records a Hunspell recognition-semantics version. Loading rejects an
artifact when its format, semantics, source fingerprints, checksum, bounds, or
parsed structure differs from the current contract.

The artifact preserves every current recognition-affecting field: lexemes and
flags; ordered prefix and suffix rules with stable IDs; continuation flags;
conditions; circumfix, forbidden-word, keep-case, and need-affix flags; and
compound configuration. The derived stem index is rebuilt during load. No
affix forms are pre-expanded, so the artifact does not turn a bounded lazy
derivation into an unbounded cache build.

`NOSUGGEST` is also retained as a suggestion-policy flag. It does not change
recognition, but it prevents the marked spelling from being returned by the
suggestion API after the cache is loaded.

It also preserves dictionary-entry and affix morphology as a private interned
string table with compact references. Morphology has no public runtime API and
does not change recognition, but retaining it makes the imported representation
lossless for later analysis features without multiplying repeated tags in
memory.

### Measured footprint

The pinned de_DE fixture has 258,219 dictionary entries and no morphology
fields. On the 64-bit CI/runtime target, retaining the empty compact slice costs
16 bytes per entry (about 3.94 MiB) and the cache's empty-field count costs four
bytes per entry (about 0.99 MiB). There are no morphology-string allocations for
that fixture. A populated field is stored once in the intern table and each use
adds only its four-byte ID to the cache.

`install` writes the cache only after a strict import succeeds. A failed strict
import leaves the verified source cache available for diagnostics but does not
create a runtime artifact. A stale or malformed runtime cache is disposable and
must be rebuilt from its source pair; it is never silently repaired in place.

## Offline runtime use

Pass an affix path to `check` or `analyze` with `--hunspell`:

```sh
ferrolex check \
  --hunspell "$HOME/.cache/ferrolex/dictionaries/pl_PL/pl_PL.aff" \
  słowami

ferrolex analyze \
  --hunspell "$HOME/.cache/ferrolex/dictionaries/pl_PL/pl_PL.aff" \
  src/lib.rs
```

The option derives the adjacent `.dic` source and versioned runtime-cache path
from the affix stem. When the cache exists, it reads all three local files and
verifies the cache against the exact `.aff` and `.dic` bytes before any lookup.
`--dictionary` and `--hunspell` may be repeated and composed in the same
invocation.

When the cache is absent, ferrolex imports the `.aff`/`.dic` pair directly in
memory, emits importer diagnostics on standard error, and prints a notice that
this path is slower. It does not try to write beside the sources, so ordinary
Hunspell pairs in read-only system directories remain usable. For repeated
use, compile the pair into a writable location and pass the result with
`--compiled`, or use `dictionary install` for a reviewed catalog locale.
Malformed or stale caches still fail closed with the same remediation choices;
they are never ignored as though absent. Runtime commands perform no network
access and never rebuild or repair a cache implicitly.

## Standalone artifact

For distribution, compile a strict Hunspell pair directly:

```sh
ferrolex compile dictionary.aff dictionary.dic -o dictionary.flexh
ferrolex validate --compiled dictionary.flexh
ferrolex check --compiled dictionary.flexh derived-form
```

The resulting `FLXHSP` artifact is self-contained: `--compiled` verifies its
format, semantics version, checksum, bounds, and parsed structure without
reading the original pair. Its embedded source digests remain descriptive
provenance, not an availability requirement. Preserve the upstream license and
notice alongside any redistributed artifact; the artifact does not alter those
obligations.
