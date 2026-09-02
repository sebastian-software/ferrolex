# Verified LibreOffice dictionary fetching

ferrolex can acquire a reviewed pair of LibreOffice Hunspell files, but it
does not bundle dictionary content and it never downloads a dictionary during
normal checking, validation, compilation, tests, or CI.

This is source acquisition, not ferrolex dictionary distribution: the optional
installer fetches raw upstream bytes only after an explicit user command and
writes them into a caller-selected local cache. ferrolex does not operate a
companion artifact repository or publish these dictionaries. A product that
redistributes a resulting compiled artifact remains responsible for the
locale-specific license evidence recorded in the catalog.

The optional `ferrolex dictionary` command is an installer, not an updater:

- no cache directory is inferred; the caller supplies `--cache`;
- only absolute `https://` source URLs are accepted;
- every `.aff` and `.dic` response is checked against a catalogued SHA-256
  digest at an immutable upstream revision;
- the bytes are verified before an atomic, non-replacing write to
  `<cache>/<locale>/<file-name>`;
- cache files and their parent directory are synced before success is reported,
  and stale temporary siblings from interrupted writes are removed later;
- filesystems without hard-link support use a serialized atomic rename followed
  by a digest re-check;
- a cache file with different bytes is preserved and causes an error;
- a cache file whose bytes match the pinned digest is reused without a network
  request;
- redirects are refused with a diagnostic that identifies the original URL and
  HTTP status; the command has no implicit retries, telemetry, or background
  updates.
- requests time out after 10 seconds while connecting, 15 seconds while
  awaiting response headers, 60 seconds while receiving a response body, or
  75 seconds end-to-end; timeout failures identify the URL and stage.

This separation ensures the spell-checking runtime remains deterministic and
offline after installation. The command never follows a branch, learns a new
digest, or replaces a cached file with different bytes. A catalog update is a
normal reviewed ferrolex release change.

## Initial LibreOffice catalog

`ferrolex dictionary list` exposes these reviewed sources:

| Locale | Pinned upstream paths | Reviewed SPDX expression | Locale-specific notice |
| --- | --- | --- | --- |
| `en_US` | `en/en_US.aff`, `en/en_US.dic` | `GPL-2.0-only` | `en/license.txt` |
| `de_DE` | `de/de_DE_frami.aff`, `de/de_DE_frami.dic` | `GPL-2.0-only OR GPL-3.0-only` | `de/README_de_DE_frami.txt` |
| `hu_HU` | `hu_HU/hu_HU.aff`, `hu_HU/hu_HU.dic` | `MPL-2.0-or-later OR LGPL-3.0-or-later` | `hu_HU/README_hu_HU.txt` |
| `es_ES` | `es/es_ES.aff`, `es/es_ES.dic` | `GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1` | `es/LICENSE.md` |
| `fr_FR` | `fr_FR/dictionaries/fr.aff`, `fr_FR/dictionaries/fr.dic` | `MPL-2.0` | `fr_FR/dictionaries/README_dict_fr.txt` |
| `it_IT` | `it_IT/it_IT.aff`, `it_IT/it_IT.dic` | `GPL-3.0-only` | `it_IT/README_it_IT.txt` |
| `pt_BR` | `pt_BR/pt_BR.aff`, `pt_BR/pt_BR.dic` | `LGPL-3.0-only OR MPL-1.1` | `pt_BR/README_pt_BR.txt` |
| `pt_PT` | `pt_PT/pt_PT.aff`, `pt_PT/pt_PT.dic` | `GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1` | `pt_PT/LICENSES.txt` |
| `nl_NL` | `nl_NL/nl_NL.aff`, `nl_NL/nl_NL.dic` | `BSD-3-Clause OR CC-BY-3.0` | `nl_NL/LICENSE.txt` |
| `pl_PL` | `pl_PL/pl_PL.aff`, `pl_PL/pl_PL.dic` | `GPL-2.0-only OR LGPL-2.1-only OR MPL-1.1 OR Apache-2.0 OR CC-BY-4.0` | `pl_PL/README_pl_PL.txt` |
| `ru_RU` | `ru_RU/ru_RU.aff`, `ru_RU/ru_RU.dic` | `BSD-3-Clause` | `ru_RU/README_ru_RU.txt` |
| `tr_TR` | `tr_TR/tr_TR.aff`, `tr_TR/tr_TR.dic` | `MPL-2.0` | `tr_TR/LICENSE` |
| `ar` | `ar/ar.aff`, `ar/ar.dic` | `GPL-2.0-or-later OR LGPL-2.1-or-later OR MPL-1.1` | `ar/COPYING.txt` |
| `uk_UA` | `uk_UA/uk_UA.aff`, `uk_UA/uk_UA.dic` | `MPL-1.1` | `uk_UA/README_uk_UA.txt` |
| `sv_SE` | `sv_SE/dictionaries/sv_SE.aff`, `sv_SE/dictionaries/sv_SE.dic` | `LGPL-3.0-only` | `sv_SE/LICENSE_sv_SE.txt` |
| `id_ID` | `id/id_ID.aff`, `id/id_ID.dic` | `LGPL-3.0-only` | `id/LICENSE-dict` |
| `hi_IN` | `hi_IN/hi_IN.aff`, `hi_IN/hi_IN.dic` | `GPL-2.0-only` | `hi_IN/COPYING` |
| `bn_BD` | `bn_BD/bn_BD.aff`, `bn_BD/bn_BD.dic` | `GPL-2.0-only` | `bn_BD/COPYING` |

All paths and file digests are pinned to LibreOffice/dictionaries commit
`f2ff99058268502bdcf4cad25c1ca2935ad8aa7d`. Each reviewed SPDX expression is
paired with its linked locale notice, rather than assigning one license to the
entire LibreOffice collection. It is a source catalog, *not* legal advice or a
claim that all dictionaries share one redistributable license. Consumers must
review and accept the linked terms for every locale they distribute.

`list` also prints the source encoding. Installation preserves upstream bytes:
`de_DE` is ISO-8859-1, `pl_PL` is ISO-8859-2, `id_ID` has an
ISO-8859-1-compatible affix file with a UTF-8 word list, and `hu_HU` has a
UTF-8-declared affix file with reviewed ISO-8859-2 fallback bytes plus a UTF-8
word list; the other initial entries are UTF-8. `validate` and `install`
decode shared UTF-8, ISO-8859-1, and ISO-8859-2 pairs losslessly from their
AFF `SET` declaration. `install` also supplies the reviewed per-file or
fallback decoding policy needed by the mixed-encoding `id_ID` and `hu_HU`
pairs.
Arabic and Turkish are deliberately available for compatibility evaluation;
recognition support remains dependent on the diagnostics from `validate --strict`.
The [locale compatibility matrix](locale-compatibility.md) records this
evidence separately for every catalog entry.

CJK locales are intentionally absent. They need a dedicated text-segmentation
contract, rather than treating a dictionary fetch as complete language support.
The pinned LibreOffice collection has no Urdu (`ur`) Hunspell pair, so Urdu is
not silently substituted with another upstream source; adding it needs a
separate provenance and license review.

## Fetch workflow

First inspect the available immutable source and its notice:

```sh
ferrolex dictionary list
```

Install a reviewed locale into an explicit cache directory:

```sh
ferrolex dictionary fetch pl_PL \
  --cache "$HOME/.cache/ferrolex/dictionaries"
```

The command prints the fetched paths plus the reviewed SPDX expression and
notice URL, then points to `dictionary install` for building the runtime cache
with the pair's reviewed provenance and encoding policy. Validate the resulting
files before using them:

```sh
ferrolex validate --strict \
  "$HOME/.cache/ferrolex/dictionaries/pl_PL/pl_PL.aff" \
  "$HOME/.cache/ferrolex/dictionaries/pl_PL/pl_PL.dic"
```

No dictionary data or network fixtures are committed in this repository. Only
source paths, provenance notices, encodings, and SHA-256 values are stored.
That preserves the engine's `MIT OR Apache-2.0` licensing while making each
consuming product responsible for the exact upstream terms it accepts.

## Checked install workflow

For the normal workflow, `install` obtains the same checked source bytes (or
reuses an already verified cache entry) and
then runs the strict byte-oriented importer in one operation:

```sh
ferrolex dictionary install pl_PL \
  --cache "$HOME/.cache/ferrolex/dictionaries"
```

The importer decodes UTF-8, ISO-8859-1, and ISO-8859-2 in memory, without
rewriting the cache bytes whose digest establishes provenance. It also handles
the catalogued `id_ID` mixed encoding pair explicitly. An unsupported
recognition directive makes `install` return exit status `1`, while leaving the
verified source cache available for diagnostic work. `fetch` remains useful
when acquisition and validation need to be separate steps.

After a successful strict import, `install` also writes a versioned,
provenance-bound runtime cache beside the affix source. See
[Hunspell runtime cache](hunspell-runtime-cache.md) for its validity and
rebuild contract.

Use the installed dictionary offline with the affix path:

```sh
ferrolex check \
  --hunspell "$HOME/.cache/ferrolex/dictionaries/pl_PL/pl_PL.aff" \
  słowami
```

This loads the adjacent runtime cache only after verifying it against both
source files. If no cache exists, `--hunspell` strictly imports an ordinary pair
directly and prints a slower-path notice plus a `compile`/`--compiled` hint.
Catalog pairs with reviewed per-file encoding overrides must go through
`dictionary install`; a locale-shaped filename alone is not treated as catalog
provenance. This makes compatible read-only system dictionary directories
usable without writing beside their sources. Checking never downloads or
silently writes a cache; importer errors and malformed or stale caches remain
errors.
