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

Each catalogued pair also has a reviewed SPDX expression and a link to its
immutable upstream notice in [license evidence](#license-evidence). This is
provenance for the exact source pair, not legal advice or a blanket statement
about the LibreOffice collection.

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
| `es_ES` | Latin | UTF-8 | Strict fixture | The exact source uses a variation-selector UTF-8 flag. The fixture covers a stem, suffix form, and Unicode-default casing fallback; `TRY` remains a suggestion-only warning. |
| `fr_FR` | Latin | UTF-8 | Strict fixture | The exact pair accepts its multi-scalar, anchored `BREAK` patterns and the fixture covers stem, suffix, casing, and `KEEPCASE` behavior. `TRY` is a suggestion-only warning; the source count discrepancy remains visible as a warning. |
| `it_IT` | Latin | UTF-8 | Strict fixture | Slash-prefixed DIC comments, escaped literal slashes, and morphology-only trailing slashes import without changing recognition. `HOME`, `NAME`, `VERSION`, and `TRY` are suggestion-only warnings; `LANG it_IT` uses Unicode-default capitalization fallback. |
| `pt_BR` | Latin | UTF-8 with leading BOM in `.aff` and `.dic` | Strict fixture | The byte importer normalizes each leading BOM. The fixture covers stem, suffix, and casing; `TRY`, `MAXNGRAMSUGS`, `MAXDIFF`, `ONLYMAXDIFF`, and `WARN` are suggestion-only warnings. |
| `pt_PT` | Latin | UTF-8 | Strict fixture | The exact pair uses `LANG pt_PT` with Unicode-default capitalization fallback and has stem, suffix, and casing probes. `TRY` is a suggestion-only warning. |
| `nl_NL` | Latin | UTF-8 | Strict fixture | Grouped `COMPOUNDRULE` flags, `COMPOUNDMIN 0`, escaped DIC slashes, morphology-only trailing slashes, and `KEEPCASE` are covered. The fixture includes a generated compound; suggestion directives remain warnings. |
| `pl_PL` | Latin | ISO-8859-2 | Strict fixture | The exact-byte fixture imports through the public ISO-8859-2 byte path, compiles and reloads the runtime cache, and covers stem, suffix, and casing probes. Count discrepancies remain warning-only. |
| `ru_RU` | Cyrillic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports, builds the runtime cache, accepts `русский` and `русского`, and rejects `русскии`. `TRY` is the only observed suggestion-only warning; the probes do not establish full upstream recognition parity. |
| `tr_TR` | Latin, agglutinative | UTF-8 | Strict fixture | The digest-verified exact pair strict-imports with `FLAG num`, including flag `0`, entries up to 32 KiB, and up to 4,096 flags per entry. It accepts the recorded `yedieminle` suffix form, and `LANG tr_TR` uses dotted/dotless-I capitalization fallback. The probes are not full generated-form or recognition parity evidence. |
| `ar` | Arabic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports with `FLAG long`, long-form `AF` aliases, and alias references in affix continuation flags. `TRY`, `KEY`, and `MAP` remain suggestion-only warnings; this is not full upstream recognition parity. |
| `uk_UA` | Cyrillic | UTF-8 | Strict fixture | The 2026-08-11 exact-pair probe strict-imports with literal `BREAK`, `ICONV`, `IGNORE`, `WORDCHARS`, bounded negative lookbehind, and start-anchored affix conditions. This is import evidence only, not full upstream recognition parity. |
| `sv_SE` | Latin | UTF-8 | Blocked | Its anchored `BREAK` pattern and `ONLYMAXDIFF` remain strict errors. Compound safeguards, compound-position flags, `FULLSTRIP`, and `WORDCHARS` strict-import. |
| `id_ID` | Latin | AFF ISO-8859-1; DIC UTF-8 | Blocked | `dictionary install` uses the reviewed per-file encoding override without rewriting cached bytes. `WORDCHARS` is an additional strict blocker. |
| `hi_IN` | Devanagari | UTF-8 with leading BOM in `.aff` | Blocked | The byte importer normalizes the leading BOM; `BREAK`, `ICONV`, and `WORDCHARS` remain outside the subset. |
| `bn_BD` | Bengali | UTF-8 | Blocked | The 2026-08-11 exact pair reports source-aware strict errors for its `$`-suffixed SFX header counts (starting at AFF line 142) and empty DIC flag sections (for example line 87,465), rather than silently importing them. `ICONV` strict-imports; no reviewed strict fixture or recognition probes exist yet. |

`No known AFF error` is purposefully weaker than "strict validation passes".
For example, strict mode also checks malformed affix rules, dictionary entries,
declared encoding, size limits, and flags. Conversely, a blocked locale may
still be useful in lenient mode for diagnostic exploration, but lenient output
must never be presented as equivalent dictionary recognition.

## License evidence

These expressions are reviewed against the named notice at the pinned source
revision. `OR` records an upstream license choice; consumers that redistribute
a dictionary must select and comply with one permitted option.

| Locale | Reviewed SPDX expression | Immutable upstream notice |
| --- | --- | --- |
| `en_US` | `GPL-2.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/en/license.txt) |
| `de_DE` | `GPL-2.0-only OR GPL-3.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/de/README_de_DE_frami.txt) |
| `hu_HU` | `MPL-2.0-or-later OR LGPL-3.0-or-later` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/hu_HU/README_hu_HU.txt) |
| `es_ES` | `GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/es/LICENSE.md) |
| `fr_FR` | `MPL-2.0` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/fr_FR/dictionaries/README_dict_fr.txt) |
| `it_IT` | `GPL-3.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/it_IT/README_it_IT.txt) |
| `pt_BR` | `LGPL-3.0-only OR MPL-1.1` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/pt_BR/README_pt_BR.txt) |
| `pt_PT` | `GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/pt_PT/LICENSES.txt) |
| `nl_NL` | `BSD-3-Clause OR CC-BY-3.0` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/nl_NL/LICENSE.txt) |
| `pl_PL` | `GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1 OR Apache-2.0 OR CC-BY-4.0` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/pl_PL/README_pl_PL.txt) |
| `ru_RU` | `BSD-3-Clause` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/ru_RU/README_ru_RU.txt) |
| `tr_TR` | `MPL-2.0` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/tr_TR/LICENSE) |
| `ar` | `GPL-2.0-or-later OR LGPL-2.1-or-later OR MPL-1.1` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/ar/COPYING.txt) |
| `uk_UA` | `MPL-1.1` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/uk_UA/README_uk_UA.txt) |
| `sv_SE` | `LGPL-3.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/sv_SE/LICENSE_sv_SE.txt) |
| `id_ID` | `LGPL-3.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/id/LICENSE-dict) |
| `hi_IN` | `GPL-2.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/hi_IN/COPYING) |
| `bn_BD` | `GPL-2.0-only` | [Notice](https://raw.githubusercontent.com/LibreOffice/dictionaries/f2ff99058268502bdcf4cad25c1ca2935ad8aa7d/bn_BD/COPYING) |

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
scripts/fetch-compat-fixtures.sh --set scorecard /tmp/ferrolex-compat-fixtures
FERROLEX_COMPAT_FIXTURES=/tmp/ferrolex-compat-fixtures \
FERROLEX_COMPAT_FIXTURE_SET=scorecard \
FERROLEX_COMPAT_ORACLE=hunspell \
FERROLEX_COMPAT_SCORECARD=/tmp/recognition-scorecard.tsv \
FERROLEX_COMPAT_SCORECARD_BASELINE=crates/ferrolex-hunspell/tests/real_world/scorecard-baseline.tsv \
  cargo test -p ferrolex-hunspell --test real_world -- --nocapture
```

Weekly, manual, and release compatibility runs invoke Hunspell for the seven
scorecard fixtures and compare their deterministic rows with the checked-in
baseline. Each artifact records the corpus recipe, oracle command, and oracle
version as well as the per-locale agreements, disagreements, and a per-word
outcome digest. The 150-minute job limit is based on the preceding 107m10s
all-fixture measurement and leaves diagnostic/artifact headroom; it does not
claim a faster seven-locale run. The scorecard is evidence for its recorded
corpus, not a general Hunspell-parity assertion.
