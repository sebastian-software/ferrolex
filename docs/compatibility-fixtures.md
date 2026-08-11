# Real-world compatibility fixtures

ferrolex does not redistribute third-party dictionaries. The real-world
compatibility suite consumes only locally supplied copies selected by an
explicit opt-in environment variable. This keeps the engine's `MIT OR
Apache-2.0` license separate from each dictionary's license and makes every
source review visible.

## Current fixture manifest

[`crates/ferrolex-hunspell/tests/real_world/manifest.tsv`](../crates/ferrolex-hunspell/tests/real_world/manifest.tsv)
is the authoritative registry. It pins a source revision, file sizes,
SHA-256 values, decoding status, import expectation, category-labelled positive
recognition probes, and a negative probe for every fixture. It deliberately
contains only this bounded metadata and probe set, never dictionary source data
or affix rules.

`hu_HU.aff` declares UTF-8 but has isolated ISO-8859-2 legacy bytes and NEL
line separators. Its manifest entry therefore uses the reviewed
`utf-8+iso-8859-2-fallback` decoder for the AFF file only; the paired DIC stays
strict UTF-8. This is a source-specific import boundary, not a relaxed UTF-8
mode for arbitrary dictionaries.

### Hungarian compatibility boundary

`hu_HU` now imports in lenient mode and protects stored (`szó`, `ház`), affixed
(`házban`), and compound (`házszó`) recognition through the runtime-cache
roundtrip. Strict import remains intentionally disabled until these observed
source constructs have their project-owned semantics:

- Hungarian-specific compound positions and restrictions
  (`COMPOUNDFIRST`, `COMPOUNDLAST`, `COMPOUNDROOT`, `ONLYROOT`, and
  `HU_KOTOHANGZO`) remain in the directive-completeness epic [#6](https://github.com/sebastian-software/ferrolex/issues/6);
- multi-scalar `BREAK` patterns are tracked by [#24](https://github.com/sebastian-software/ferrolex/issues/24);
- header metadata (`HOME`, `NAME`, `VERSION`) is tracked by [#26](https://github.com/sebastian-software/ferrolex/issues/26);
- Hungarian-specific recognition directives (`HU_KOTOHANGZO`, `ONLYROOT`,
  `SUBSTANDARD`, `GENERATE`, `LEMMA_PRESENT`, and `SYLLABLENUM`) remain in
  [#6](https://github.com/sebastian-software/ferrolex/issues/6) until their
  semantics are documented; and
- the pinned DIC declares 97,663 entries but has malformed/empty rows, while
  one `REP` row is malformed. ferrolex retains the safely parsed subset and
  reports these as `count`, `entry`, and `REP` diagnostics; the bounded format
  policy belongs to [#27](https://github.com/sebastian-software/ferrolex/issues/27).

This list is the explicit strict-import boundary for the locale. It is not an
oracle-parity claim for unlisted Hungarian forms.

The current reviewed LibreOffice fixtures are `en_US`, `de_DE`, `fr_FR`,
`nl_NL`, `hu_HU`, `ar`, and `tr_TR`. Their source-specific license evidence is
the final column of the manifest. Those terms govern the dictionary data, not
ferrolex. They are acceptable for an opt-in test because no third-party content
is distributed by this repository. Adding a fixture still requires a review of
the exact source's embedded license or notice; a package-level or hosting-site
license alone is insufficient.

### Probe categories

The positive-probe field uses semicolon-separated `category=word` entries. The
categories make the intended recognition mechanism visible in code review and
give failed probes a precise documentation link:

| Category | Contract | Failure reference |
| --- | --- | --- |
| `stem` | Stored dictionary entry. | This document |
| `affixed` | Prefix or suffix recognition derived from a stored stem. | [Affix semantics](affix-semantics.md) |
| `compound` | Recognition through declared compound rules or positions. | [Compound semantics](compound-semantics.md) |
| `sentence-initial` | Initial-capital lookup through Hunspell-style casing fallback. | [Hunspell import contract](hunspell-format.md) |
| `all-caps` | All-uppercase lookup through Hunspell-style casing fallback. | [Hunspell import contract](hunspell-format.md) |
| `keepcase` | A dictionary entry marked by `KEEPCASE`; casing fallback must not admit it. | [Hunspell import contract](hunspell-format.md) |

Every strict fixture must carry `stem` and `affixed` probes. If its AFF file
declares a compound directive, it must also carry `compound`. A fixture with
cased text must carry both casing categories. `KEEPCASE` is required only when
the pinned dictionary actually uses its declared flag. The current `de_DE`
AFF file declares `KEEPCASE w`, but its pinned dictionary has no `w`-flagged
entry; the suite reports that fact rather than claiming a non-existent probe.
Arabic has neither a cased script nor a `COMPOUND*` directive in its pinned
AFF file, so its strict fixture records a stem and an affixed clitic form only.

## Preparing fixtures locally

Create the following outside the repository or under the ignored
`.compat-fixtures` directory. Obtain the files from the revision-pinned URLs in
the manifest; the test harness intentionally has no downloader.

```text
.compat-fixtures/
├── en_US/
│   ├── en_US.aff
│   └── en_US.dic
├── de_DE/
│   ├── de_DE_frami.aff
│   └── de_DE_frami.dic
├── fr_FR/
│   ├── fr.aff
│   └── fr.dic
├── nl_NL/
│   ├── nl_NL.aff
│   └── nl_NL.dic
├── hu_HU/
│   ├── hu_HU.aff
│   └── hu_HU.dic
├── ar/
│   ├── ar.aff
│   └── ar.dic
└── tr_TR/
    ├── tr_TR.aff
    └── tr_TR.dic
```

Before enabling the suite, compare both byte size and SHA-256 with the
manifest. The harness repeats both checks before importing. On macOS and most
Linux installations, the optional manual check is:

```sh
shasum -a 256 .compat-fixtures/en_US/en_US.aff .compat-fixtures/en_US/en_US.dic
shasum -a 256 .compat-fixtures/de_DE/de_DE_frami.aff .compat-fixtures/de_DE/de_DE_frami.dic
shasum -a 256 .compat-fixtures/fr_FR/fr.aff .compat-fixtures/fr_FR/fr.dic
shasum -a 256 .compat-fixtures/nl_NL/nl_NL.aff .compat-fixtures/nl_NL/nl_NL.dic
shasum -a 256 .compat-fixtures/hu_HU/hu_HU.aff .compat-fixtures/hu_HU/hu_HU.dic
shasum -a 256 .compat-fixtures/ar/ar.aff .compat-fixtures/ar/ar.dic
shasum -a 256 .compat-fixtures/tr_TR/tr_TR.aff .compat-fixtures/tr_TR/tr_TR.dic
```

Then run the opt-in integration test:

```sh
FERROLEX_COMPAT_FIXTURES="$PWD/.compat-fixtures" \
  cargo test -p ferrolex-hunspell --test real_world -- --nocapture
```

Without `FERROLEX_COMPAT_FIXTURES`, the fixture test prints a skip message and
returns successfully; normal developer and CI runs therefore never require
network access, licensed data, or a local office installation.

## Feature report

With fixtures enabled, the test prints one report block per source containing:

- pinned source and data-license label;
- the recorded SHA-256 values and local byte-length check;
- all directives observed in the `.aff` file;
- the recognition-affecting directives emitted by the ferrolex importer;
- positive and negative probe outcomes after a runtime-cache roundtrip; or an
  explicit encoding boundary.

This is a compatibility scorecard, not a claim of bug-for-bug Hunspell parity.
When a recognition directive becomes supported, update the project-owned
format and semantic documentation, rerun this report, and add a focused
independent fixture before changing the real-world baseline.

## Differential recognition scorecard

The CI compatibility job downloads the digest-pinned reference files into its
temporary directory, installs the system `hunspell` command, and uploads
`recognition-scorecard.tsv` as an artifact. The normal workspace test remains
offline and does not require this oracle. To reproduce the artifact locally:

```sh
scripts/fetch-compat-fixtures.sh /tmp/ferrolex-compat-fixtures
FERROLEX_COMPAT_FIXTURES=/tmp/ferrolex-compat-fixtures \
FERROLEX_COMPAT_ORACLE=hunspell \
FERROLEX_COMPAT_SCORECARD=/tmp/recognition-scorecard.tsv \
  cargo test -p ferrolex-hunspell --test real_world -- --nocapture
```

The corpus combines stored stems with explicit manifest probes (including
generated-form and casing cases) and deterministic negative mutations. The
oracle is development-only black-box observation; ferrolex production code has
no dependency on Hunspell or Nuspell.
