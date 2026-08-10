# Verified LibreOffice dictionary fetching

ferrolex can acquire a reviewed pair of LibreOffice Hunspell files, but it
does not bundle dictionary content and it never downloads a dictionary during
normal checking, validation, compilation, tests, or CI.

The optional `ferrolex dictionary` command is an installer, not an updater:

- no cache directory is inferred; the caller supplies `--cache`;
- only absolute `https://` source URLs are accepted;
- every `.aff` and `.dic` response is checked against a catalogued SHA-256
  digest at an immutable upstream revision;
- the bytes are verified before an atomic write to
  `<cache>/<locale>/<file-name>`;
- a cache file with different bytes is preserved and causes an error;
- redirects are refused; the command has no implicit retries, telemetry, or
  background updates.

This separation ensures the spell-checking runtime remains deterministic and
offline after installation. The command never follows a branch, learns a new
digest, or replaces a cached file with different bytes. A catalog update is a
normal reviewed ferrolex release change.

## Initial LibreOffice catalog

`ferrolex dictionary list` exposes these reviewed sources:

| Locale | Pinned upstream paths | Locale-specific notice |
| --- | --- | --- |
| `en_US` | `en/en_US.aff`, `en/en_US.dic` | `en/license.txt` |
| `de_DE` | `de/de_DE_frami.aff`, `de/de_DE_frami.dic` | `de/README_de_DE_frami.txt` |
| `es_ES` | `es/es_ES.aff`, `es/es_ES.dic` | `es/LICENSE.md` |
| `fr_FR` | `fr_FR/dictionaries/fr.aff`, `fr_FR/dictionaries/fr.dic` | `fr_FR/dictionaries/README_dict_fr.txt` |
| `it_IT` | `it_IT/it_IT.aff`, `it_IT/it_IT.dic` | `it_IT/README_it_IT.txt` |
| `pt_BR` | `pt_BR/pt_BR.aff`, `pt_BR/pt_BR.dic` | `pt_BR/README_pt_BR.txt` |
| `pt_PT` | `pt_PT/pt_PT.aff`, `pt_PT/pt_PT.dic` | `pt_PT/LICENSES.txt` |
| `nl_NL` | `nl_NL/nl_NL.aff`, `nl_NL/nl_NL.dic` | `nl_NL/LICENSE.txt` |
| `pl_PL` | `pl_PL/pl_PL.aff`, `pl_PL/pl_PL.dic` | `pl_PL/README_pl_PL.txt` |
| `ru_RU` | `ru_RU/ru_RU.aff`, `ru_RU/ru_RU.dic` | `ru_RU/README_ru_RU.txt` |
| `tr_TR` | `tr_TR/tr_TR.aff`, `tr_TR/tr_TR.dic` | `tr_TR/LICENSE` |
| `ar` | `ar/ar.aff`, `ar/ar.dic` | `ar/COPYING.txt` |
| `uk_UA` | `uk_UA/uk_UA.aff`, `uk_UA/uk_UA.dic` | `uk_UA/README_uk_UA.txt` |
| `sv_SE` | `sv_SE/dictionaries/sv_SE.aff`, `sv_SE/dictionaries/sv_SE.dic` | `sv_SE/LICENSE_sv_SE.txt` |
| `id_ID` | `id/id_ID.aff`, `id/id_ID.dic` | `id/LICENSE-dict` |
| `hi_IN` | `hi_IN/hi_IN.aff`, `hi_IN/hi_IN.dic` | `hi_IN/COPYING` |
| `bn_BD` | `bn_BD/bn_BD.aff`, `bn_BD/bn_BD.dic` | `bn_BD/COPYING` |

All paths and file digests are pinned to LibreOffice/dictionaries commit
`f2ff99058268502bdcf4cad25c1ca2935ad8aa7d`. The catalog treats the linked
locale notice as the source-specific licensing evidence, instead of assigning
one license to the entire LibreOffice collection. It is a source catalog, *not*
a claim that all dictionaries share one redistributable license. Consumers must
review and accept the linked terms for every locale they distribute.

`list` also prints the source encoding. Installation preserves upstream bytes:
`de_DE` is ISO-8859-1, `pl_PL` is ISO-8859-2, and `id_ID` has an
ISO-8859-1-compatible affix file with a UTF-8 word list; the other initial
entries are UTF-8. The current `validate` command accepts UTF-8 Hunspell input,
so legacy pairs need an explicit, lossless transcode before import. Arabic and
Turkish are deliberately available for compatibility evaluation; recognition
support remains dependent on the diagnostics from `validate --strict`.
The [locale compatibility matrix](locale-compatibility.md) records this
evidence separately for every catalog entry.

CJK locales are intentionally absent. They need a dedicated text-segmentation
contract, rather than treating a dictionary fetch as complete language support.
The pinned LibreOffice collection has no Urdu (`ur`) Hunspell pair, so Urdu is
not silently substituted with another upstream source; adding it needs a
separate provenance and license review.

## Install workflow

First inspect the available immutable source and its notice:

```sh
ferrolex dictionary list
```

Install a reviewed locale into an explicit cache directory:

```sh
ferrolex dictionary fetch de_DE \
  --cache "$HOME/.cache/ferrolex/dictionaries"
```

The command prints the installed paths plus the source license label and
notice URL. Validate the resulting files before using them:

```sh
ferrolex validate --strict \
  "$HOME/.cache/ferrolex/dictionaries/de_DE/de_DE_frami.aff" \
  "$HOME/.cache/ferrolex/dictionaries/de_DE/de_DE_frami.dic"
```

No dictionary data or network fixtures are committed in this repository. Only
source paths, provenance notices, encodings, and SHA-256 values are stored.
That preserves the engine's `MIT OR Apache-2.0` licensing while making each
consuming product responsible for the exact upstream terms it accepts.

## Checked install workflow

For the normal workflow, `install` fetches the same checked source bytes and
then runs the strict byte-oriented importer in one operation:

```sh
ferrolex dictionary install de_DE \
  --cache "$HOME/.cache/ferrolex/dictionaries"
```

The importer decodes UTF-8, ISO-8859-1, and ISO-8859-2 in memory, without
rewriting the cache bytes whose digest establishes provenance. It also handles
the catalogued `id_ID` mixed encoding pair explicitly. An unsupported
recognition directive makes `install` return exit status `1`, while leaving the
verified source cache available for diagnostic work. `fetch` remains useful
when acquisition and validation need to be separate steps.
