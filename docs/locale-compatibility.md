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
- **Strict probe** records one current strict import of the digest-verified
  local source pair. It establishes neither recognition probes nor a reusable
  fixture.
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
| `en_US` | Latin | UTF-8 | Strict probe | The 2026-08-11 digest-verified exact pair strict-imports after bounded `COMPOUNDRULE` quantifier support. `TRY` and `NOSUGGEST` remain suggestion-only warnings. Recognition probes and an opt-in fixture remain required before a broader support claim. |
| `de_DE` | Latin | ISO-8859-1 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports and builds the runtime cache. It accepts `Straße`, `Häuser`, and `Häusern`, and rejects `Ferrolex`. `TRY`, `MAP`, and `NOSUGGEST` remain suggestion-only warnings; this is not full upstream recognition parity. |
| `es_ES` | Latin | UTF-8 | Strict probe | The 2026-08-11 digest-verified exact pair strict-imports with a variation-selector UTF-8 flag. `MAP` and `TRY` remain suggestion-only warnings. Recognition probes and an opt-in fixture remain required before a broader support claim. |
| `fr_FR` | Latin | UTF-8 | Blocked | `FULLSTRIP`, `ICONV`, `OCONV`, and `WORDCHARS` strict-import. Its anchored, multi-scalar `BREAK` patterns remain recognition-affecting errors; `TRY`, `MAP`, `KEY`, and `NOSUGGEST` are suggestion-only warnings. |
| `it_IT` | Latin | UTF-8 | Blocked | `HOME`, `NAME`, and `VERSION` are not part of the importer subset. `LANG` imports with Unicode-default capitalization fallback. |
| `pt_BR` | Latin | UTF-8 with leading BOM in `.aff` | Blocked | The byte importer normalizes the leading BOM; `BREAK`, `ONLYMAXDIFF`, and `WARN` remain outside the subset. |
| `pt_PT` | Latin | UTF-8 | Strict probe | The 2026-08-11 digest-verified exact pair strict-imports after `LANG` support. Its tag uses Unicode-default capitalization fallback; `KEY`, `MAP`, and `TRY` remain suggestion-only warnings. Recognition probes and an opt-in fixture remain required before a broader support claim. |
| `nl_NL` | Latin | UTF-8 | Blocked | `ONLYMAXDIFF`, `WARN`, `FORCEUCASE`, non-positive `COMPOUNDMIN`, `CHECKCOMPOUND*`, and an oversized/complex `BREAK` section remain strict errors. `OCONV`, `WORDCHARS`, and bounded `COMPOUNDRULE` are no longer gates. |
| `pl_PL` | Latin | ISO-8859-2 | Strict fixture | The exact-byte fixture imports through the public ISO-8859-2 byte path, compiles and reloads the runtime cache, accepts `słowo` and `słowami`, and rejects its synthetic negative probe. This validates the documented subset, not full Hunspell compatibility or redistribution terms. |
| `ru_RU` | Cyrillic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports, builds the runtime cache, accepts `русский` and `русского`, and rejects `русскии`. `TRY` is the only observed suggestion-only warning; the probes do not establish full upstream recognition parity. |
| `tr_TR` | Latin, agglutinative | UTF-8 | Blocked | `LANG tr_TR` uses dotted/dotless-I capitalization fallback. The exact pair remains blocked by importer limits on exceptionally large flag sections and lines; generated-form probes are still required before any support claim. |
| `ar` | Arabic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports with `FLAG long`, long-form `AF` aliases, and alias references in affix continuation flags. `TRY`, `KEY`, and `MAP` remain suggestion-only warnings; this is not full upstream recognition parity. |
| `uk_UA` | Cyrillic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports with literal `BREAK`, `ICONV`, `IGNORE`, `WORDCHARS`, bounded negative lookbehind, and start-anchored affix conditions. This is import evidence only, not full upstream recognition parity. |
| `sv_SE` | Latin | UTF-8 | Blocked | Its anchored `BREAK` pattern, `CHECKCOMPOUNDTRIPLE`, `SIMPLIFIEDTRIPLE`, `CHECKCOMPOUNDDUP`, `ONLYMAXDIFF`, `FORCEUCASE`, and `CHECKCOMPOUNDREP` remain strict errors. Compound-position flags, `FULLSTRIP`, and `WORDCHARS` strict-import. |
| `id_ID` | Latin | AFF ISO-8859-1; DIC UTF-8 | Blocked | `dictionary install` uses the reviewed per-file encoding override without rewriting cached bytes. `WORDCHARS` is an additional strict blocker. |
| `hi_IN` | Devanagari | UTF-8 with leading BOM in `.aff` | Blocked | The byte importer normalizes the leading BOM; `BREAK`, `ICONV`, and `WORDCHARS` remain outside the subset. |
| `bn_BD` | Bengali | UTF-8 | Blocked | `ICONV` strict-imports, but the exact pair has unsupported affix-header syntax and many malformed/empty flag sections. Strict-import and recognition probes remain pending after those format boundaries are addressed. |

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

When a row becomes a strict fixture, add an opt-in local fixture that records
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


## Recognition scorecard

The optional external Hunspell oracle produces a per-locale accept/reject
scorecard. Reproduce it with:

```sh
scripts/fetch-compat-fixtures.sh /tmp/ferrolex-compat-fixtures
FERROLEX_COMPAT_FIXTURES=/tmp/ferrolex-compat-fixtures \
FERROLEX_COMPAT_ORACLE=hunspell \
FERROLEX_COMPAT_SCORECARD=/tmp/recognition-scorecard.tsv \
  cargo test -p ferrolex-hunspell --test real_world -- --nocapture
```

The CI baseline from [run 31487827770](https://github.com/sebastian-software/ferrolex/actions/runs/31487827770)
is recorded in the `recognition-scorecard` artifact.

| Locale | Status | Corpus | Agreement | Disagreement |
| --- | --- | ---: | ---: | ---: |
| `en_US` | measured | 137 | 137 | 0 |
| `de_DE` | measured | 135 | 135 | 0 |
| `fr_FR` | measured | 135 | 135 | 0 |
| `nl_NL` | measured | 135 | 61 | 74 |
| `hu_HU` | lenient local fixture validated (reviewed UTF-8/ISO-8859-2 AFF fallback; oracle pending) | — | — | — |
| `ar` | measured | 133 | 133 | 0 |
| `tr_TR` | measured | 132 | 132 | 0 |

The `nl_NL` disagreement is a triage item, not a supported-parity claim.
Every CI run uploads its current TSV; blocked sources remain visible.
