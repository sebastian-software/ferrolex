# Suggestion-quality test data

This directory holds a deliberately small, test-only quality corpus and its
review-gated regression baseline. It is not a dictionary and must never be
expanded into one.

Every corpus row must have all of the following before it is committed:

- an original source or a source whose licence, terms, and attribution have
  been recorded and approved for this exact use;
- the misspelling, intended word, locale, and context needed to evaluate it;
- a provenance statement, `review_status`, reviewer, and review date; and
- `included` or `excluded`, with a concrete exclusion rationale for an
  excluded row.

Do not copy or scrape entries from spell-checkers, word lists, search logs,
spell-check corpora, or user text unless a maintainer has documented a
compatible licence and review decision in the row. Prefer original minimal
pairs. Do not include personal data, long text, or an enumerable language
lexicon.

`suggestion-quality-baseline.tsv` is deliberately a reviewed artifact rather
than a regenerated golden file. A quality regression fails CI. Updating an
expected score requires changing that file with a rationale and maintainer
review; it must not be used to mask a regression. The initial rows are marked
`requires-maintainer-review` honestly: a maintainer changes that status to
`approved-by-maintainer` and records their identity and review date only after
the PR review that approves the data.

The corpus is used only by the integration test. `ferrolex-suggest` explicitly
excludes `tests/**` from its published package, keeping this data out of the
runtime and preventing the package from becoming a dictionary distribution
channel.
