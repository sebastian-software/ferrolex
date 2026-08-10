# Real-world compatibility fixtures

ferrolex does not redistribute third-party dictionaries. The real-world
compatibility suite consumes only locally supplied copies selected by an
explicit opt-in environment variable. This keeps the engine's `MIT OR
Apache-2.0` license separate from each dictionary's license and makes every
source review visible.

## Current fixture manifest

[`crates/ferrolex-hunspell/tests/real_world/manifest.tsv`](../crates/ferrolex-hunspell/tests/real_world/manifest.tsv)
is the authoritative registry. It pins a source revision, file sizes,
SHA-256 values, decoding status, positive recognition probes, and a negative
probe for every fixture. It deliberately contains metadata only, never
dictionary words or affix rules.

The first two cases were selected for complementary evidence rather than their
licenses alone:

| ID | Source and license evidence | Purpose | Current expected result |
| --- | --- | --- | --- |
| `en_CA` | Chromium's pinned Hunspell dictionary repository includes [`README_en_CA.txt`](https://chromium.googlesource.com/chromium/deps/hunspell_dictionaries/+/50fb79b30a2e3e512c88884152f26b255d0e4074/README_en_CA.txt), which states the permissive redistribution conditions inherited from Geoff Kuenning's word list. | A compact, real ISO-8859-1 dictionary with ordinary English affixes and compound directives. | The harness losslessly converts ISO-8859-1 to UTF-8 before calling the importer. It records recognition-affecting diagnostics such as `COMPOUNDRULE` instead of hiding them. |
| `hu_HU` | The [`Magyar Ispell COPYING`](https://github.com/laszlonemeth/magyarispell/blob/455229e26eaf5c9ed5bb7a4456c131fc0985e399/COPYING) file explicitly grants GPL-2.0-or-later, LGPL-2.1-or-later, or MPL-1.1-or-later; its project identifies LibreOffice's `hu_HU` files as the release dictionary. | A large morphology stress case required by the RFC. | The pinned `.aff` bytes are not valid UTF-8 despite their `SET` line, so the suite reports a format boundary rather than silently lossy-decoding the input. |

These terms govern the dictionary data, not ferrolex. They are acceptable for
an opt-in test because no third-party content is distributed by this repository.
Adding a fixture still requires a review of the exact source's embedded
license/notice; a package-level or hosting-site license alone is insufficient.

## Preparing fixtures locally

Create the following outside the repository or under the ignored
`.compat-fixtures` directory. Obtain the files from the revision-pinned URLs in
the manifest; the suite intentionally has no downloader.

```text
.compat-fixtures/
├── en_CA/
│   ├── en_CA.aff
│   └── en_CA.dic
└── hu_HU/
    ├── hu_HU.aff
    └── hu_HU.dic
```

Before enabling the suite, compare both byte size and SHA-256 with the
manifest. The harness repeats both checks before importing. On macOS and most
Linux installations, the optional manual check is:

```sh
shasum -a 256 .compat-fixtures/en_CA/en_CA.aff .compat-fixtures/en_CA/en_CA.dic
shasum -a 256 .compat-fixtures/hu_HU/hu_HU.aff .compat-fixtures/hu_HU/hu_HU.dic
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
- positive and negative probe outcomes; or an explicit encoding boundary.

This is a compatibility scorecard, not a claim of bug-for-bug Hunspell parity.
When a recognition directive becomes supported, update the project-owned
format and semantic documentation, rerun this report, and add a focused
independent fixture before changing the real-world baseline.
