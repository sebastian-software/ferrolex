# README and brand standard

The repository follows the Ferramenta family README standard while keeping
product-specific evidence in its own sections.

## Root README

The root README uses the GitHub variant of the generated family block. Its
badge row contains the crates.io package, docs.rs API, CI, dual license, MSRV
policy, and Codecov links. The canonical tagline is **Spell checking for text
and code**. Product prose uses the lowercase `ferrolex` wordmark, including at
the beginning of sentences.

## Crate READMEs

The repository root uses the GitHub variant for the project page. The eight
public package-specific READMEs use the compact registry variant. It contains
the Ferramenta family link and sibling links without HTML, so crates.io and
docs.rs render it consistently. Both variants show the same company footer:
the root README carries the `sebastian-software-branding` section owned by
[`@sebastian-software/standards`](https://github.com/sebastian-software/standards),
and the generator mirrors that block verbatim into the crate READMEs. The
footer is therefore never hand-edited or re-rendered locally; `standards apply`
is its only writer.

The checked-in generator is intentionally dependency-free:

```sh
python3 scripts/generate-readme-family.py \
  --current ferrolex --variant github --readme README.md --check
python3 scripts/generate-readme-family.py \
  --current ferrolex --variant registry \
  --readme crates/ferrolex-core/README.md --check
```

The family registry remains the source of truth for membership and job
wording. Until the shared family generator is published for all repositories,
this local check prevents the ferrolex README surfaces from drifting.
