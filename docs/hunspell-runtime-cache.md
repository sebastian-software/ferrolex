# Hunspell runtime cache

`ferrolex dictionary install` retains the exact verified LibreOffice source
files and derives a local runtime cache beside the affix file:

```text
<cache>/<locale>/<affix-stem>.ferrolex-hunspell-v1.flexh
```

The cache is an implementation artifact, not a dictionary distribution. The
original `.aff` and `.dic` bytes remain the authoritative, license-bearing
inputs.

## Validity contract

Format version 1 uses the `FLXHSP\0\0` marker, little-endian fields, a payload
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

`install` writes the cache only after a strict import succeeds. A failed strict
import leaves the verified source cache available for diagnostics but does not
create a runtime artifact. A stale or malformed runtime cache is disposable and
must be rebuilt from its source pair; it is never silently repaired in place.

The cache is currently prepared during installation. Loading it directly from
`check` and `analyze` is the next CLI integration step.
