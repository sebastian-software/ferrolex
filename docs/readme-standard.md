# README and brand standard

The repository follows the Ferramenta family README standard while keeping
product-specific evidence in its own sections.

## Root README

The root README uses the GitHub variant of the generated family block. Its
badge row contains the crates.io package, docs.rs API, CI, dual license, MSRV
policy, and coverage-gate links. The coverage badge is static: it states the
line-coverage threshold that the `Rust coverage` job in
[the CI workflow](../.github/workflows/ci.yml) enforces, and links to that
workflow. Raising the threshold means editing `COVERAGE_MIN_LINES` in that
job and the badge text in the README. The canonical tagline is **Spell
checking for text and code**. Product prose uses the lowercase `ferrolex`
wordmark, including at the beginning of sentences.

## Package READMEs

The repository root uses the GitHub variant for the project page. The eight
public crate READMEs and the `@ferrolex/node` package README use the compact
registry variant: the Ferramenta family link and the sibling links without HTML
or tables, so crates.io, docs.rs, and npm render it consistently. The
unpublished FFI, LSP, and Python crates have no registry page and carry no
block.

The root README and the eight crate READMEs also show the same company footer.
The root carries the `sebastian-software-branding` section owned by
[`@sebastian-software/standards`](https://github.com/sebastian-software/standards),
and `scripts/mirror-readme-footer.py` mirrors that block verbatim into the crate
READMEs. The footer is therefore never hand-edited or re-rendered locally;
`standards apply` is its only writer.

## The generated family block

Family membership, job wording, grouping, and links come from `src/family.ts`
in [ferramenta](https://github.com/sebastian-software/ferramenta), the single
source of truth for the family. This repository does not maintain a second copy
of that data: `scripts/generate-readme-family.sh` runs the `ferramenta-readme`
generator from a pinned commit and writes or verifies every README surface.

```sh
scripts/generate-readme-family.sh          # re-render the block everywhere
scripts/generate-readme-family.sh --check  # exits 1 on drift; runs in CI
python3 scripts/mirror-readme-footer.py --check
```

The generator pin lives in one place, `generator_ref` in
`scripts/generate-readme-family.sh`. A registry change in ferramenta reaches
this repository by bumping that SHA and re-running the script, so the diff
shows exactly what the registry moved. Pinning also keeps the check
reproducible: a branch ref would re-verify against whatever landed upstream
since.

`--check` compares content, not whitespace, so a Markdown formatter that pads
table cells does not report drift.
