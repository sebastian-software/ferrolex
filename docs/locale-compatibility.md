# LibreOffice locale compatibility matrix

This page records what the reviewed LibreOffice source catalog establishes and
what it does **not** establish. It is an evidence matrix, not a claim that an
installed locale has the same recognition behaviour as LibreOffice or Hunspell.

Every row names exactly the `.aff`/`.dic` pair in the digest-pinned catalog at
LibreOffice/dictionaries commit
`f2ff99058268502bdcf4cad25c1ca2935ad8aa7d`. The catalog verifies the two
source-byte digests during `ferrolex dictionary fetch`; it does not download a
license notice and it does not validate a dictionary as part of fetching. See
[Dictionary fetching](dictionary-fetching.md) for the acquisition contract and
[Hunspell import contract](hunspell-format.md) for the import subset.

## How to read the status

The columns deliberately use different kinds of evidence:

- **Source encoding** is a checked catalog fact. Fetching preserves those
  bytes, so this column is not an assertion that the pair is already usable by
  a particular importer entry point.
- **AFF strict gate** is a static scan of the pinned `.aff` file against the
  documented importer subset. `No known AFF error` means that the file uses
  only supported recognition directives or suggestion-only directives. It is
  not a successful full-pair import, dictionary-entry audit, or recognition
  result.
- **Known boundary** identifies why `validate --strict` must currently be
  expected to reject the pair, or what still needs live evidence. Suggestion
  directives alone produce warnings and do not fail strict mode; every other
  unimplemented directive can affect recognition and is an error.

The matrix intentionally has no general `supported` checkmark yet. A strict
fixture is evidence only for the documented ferrolex subset and its recorded
probes; broader support also needs a completed source-license review and
language-relevant behavior coverage. This keeps a source catalog from turning
into an accidental quality promise.

## Pinned source matrix

| Locale | Script / practical group | Source encoding | AFF strict gate | Known boundary |
| --- | --- | --- | --- | --- |
| `en_US` | Latin | UTF-8 | Blocked | `COMPOUNDRULE`, `ONLYINCOMPOUND`, and `WORDCHARS` are outside the recognition subset. |
| `de_DE` | Latin | ISO-8859-1 | Strict candidate | The byte importer decodes ISO-8859-1, recognizes bounded positioned compound affixes, handles its literal `-`/`.` breaks, supports its `CHECKSHARPS`/`KEEPCASE` uppercase-`SS` behavior, and retains `WORDCHARS` metadata. A pinned-source smoke test accepts `Straße`, `Häuser`, and `Häusern`; it does not establish full upstream recognition parity. |
| `es_ES` | Latin | UTF-8 | Blocked | Its multi-scalar `PFX` flag declarations are outside the current flag model. `MAP`, `REP`, and `TRY` are suggestion-only warnings, but do not remove that recognition gate. |
| `fr_FR` | Latin | UTF-8 | Blocked | `BREAK`, `FULLSTRIP`, `ICONV`, `OCONV`, and `WORDCHARS` are recognition-affecting omissions. |
| `it_IT` | Latin | UTF-8 | Blocked | `HOME`, `LANG`, `NAME`, and `VERSION` are not part of the importer subset. |
| `pt_BR` | Latin | UTF-8 with leading BOM in `.aff` | Blocked | The byte importer normalizes the leading BOM; `BREAK`, `ONLYMAXDIFF`, and `WARN` remain outside the subset. |
| `pt_PT` | Latin | UTF-8 | Blocked | `LANG` and `WORDCHARS` are outside the recognition subset; `KEY`, `MAP`, `REP`, and `TRY` are only suggestion warnings. |
| `nl_NL` | Latin | UTF-8 | Blocked | Compound-position and compound-check directives, `COMPOUNDRULE`, `FORCEUCASE`, `OCONV`, and `WORDCHARS` exceed the subset. |
| `pl_PL` | Latin | ISO-8859-2 | Strict fixture | The exact-byte fixture imports through the public ISO-8859-2 byte path, compiles and reloads the runtime cache, accepts `słowo` and `słowami`, and rejects its synthetic negative probe. This validates the documented subset, not full Hunspell compatibility or redistribution terms. |
| `ru_RU` | Cyrillic | UTF-8 | No known AFF error | The observed AFF uses `SET`, `SFX`, and suggestion-only `TRY`; a local exact-pair strict run succeeds with one warning. Recognition probes are still pending. |
| `tr_TR` | Latin, agglutinative | UTF-8 | Blocked | `LANG` is not implemented. Turkish should also receive generated-form probes before any support claim. |
| `ar` | Arabic | UTF-8 | Blocked | `AF`/`AM` aliases, `ICONV`, and `IGNORE` are not implemented. Fetching is useful for compatibility work, not yet a strict-import endorsement. |
| `uk_UA` | Cyrillic | UTF-8 | Blocked | `BREAK`, `ICONV`, `IGNORE`, and `WORDCHARS` are outside the subset. |
| `sv_SE` | Latin | UTF-8 | Blocked | Compound-position/check directives, `COMPOUNDRULE`, `FORCEUCASE`, `FULLSTRIP`, and `WORDCHARS` are outside the subset. |
| `id_ID` | Latin | AFF ISO-8859-1; DIC UTF-8 | Blocked | `dictionary install` uses the reviewed per-file encoding override without rewriting cached bytes. `WORDCHARS` is an additional strict blocker. |
| `hi_IN` | Devanagari | UTF-8 with leading BOM in `.aff` | Blocked | The byte importer normalizes the leading BOM; `BREAK`, `ICONV`, and `WORDCHARS` remain outside the subset. |
| `bn_BD` | Bengali | UTF-8 | Blocked | `ICONV` is outside the importer subset; strict-import and recognition probes remain pending. |

`No known AFF error` is purposefully weaker than "strict validation passes".
For example, strict mode also checks malformed affix rules, dictionary entries,
declared encoding, size limits, and flags. Conversely, a blocked locale may
still be useful in lenient mode for diagnostic exploration, but lenient output
must never be presented as equivalent dictionary recognition.

## Reproducing and advancing a row

Use an explicit cache, then validate the installed exact bytes. For a current
strict-import candidate, prefer `install` with `pl_PL`:

```sh
ferrolex dictionary install pl_PL --cache .ferrolex-dictionaries
```

Rows marked **Blocked** intentionally return exit status `1` from `install`.
That result is diagnostic evidence, not a failed download.

For shared legacy encodings, `validate` and `install` select the lossless
decoder from the AFF `SET` declaration. Never relabel or rewrite the fetched
cache in place: the cache digest is evidence for the upstream bytes, while the
importer decodes only in memory. `id_ID` is a special case because its AFF and
DIC use different encodings; its catalogued `install` path supplies the
reviewed per-file override.

When a row becomes a strict candidate, add an opt-in local fixture that records
the source revision, both source digests, decoding, accepted probes, rejected
probes, and the source-specific license evidence. The existing
[real-world fixture procedure](compatibility-fixtures.md) is the required
format for that evidence. Do not commit third-party dictionary contents merely
to make the matrix green.

## Explicit scope limits

CJK locales are not catalogued or represented by this matrix. Chinese,
Japanese, and Korean need a dedicated text-segmentation contract before a
Hunspell pair can be described as text-checking support.

Urdu is also absent, for a different reason: the pinned LibreOffice collection
has no Urdu Hunspell pair. It must not be silently substituted with a different
source; adding it requires its own provenance, license, encoding, and
compatibility review.
