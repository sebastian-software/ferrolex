# Suggestions

`ferrolex-suggest` is deliberately separate from `Dictionary::contains`.
Candidate sources enumerate stable lexical candidates; a generic dictionary is
not assumed to be enumerable. The first implementation uses Unicode-scalar
optimal-string-alignment distance, including adjacent transpositions. Candidate
generation and ranking are separate deterministic stages.

`WordList`, `UserDictionary`, and `HunspellDictionary` are candidate sources.
A mutable `UserDictionary` is snapshotted before suggestion work begins, so
project-word updates never hold its lock across edit-distance comparisons.
Hunspell candidates start with stored stems in UTF-8 byte order. For a
query-aligned near-miss stem, ferrolex also performs a bounded local affix
expansion and checks bounded query split positions for valid compounds. It
never pre-expands a dictionary: both the number of generated forms per seed and
the number of compound splits are fixed internal limits, and emitted forms use
the caller's ordinary candidate and edit-cell budgets. Thus an incomplete
result remains explicit through `Completeness`.

Before ranking, a Hunspell source filters stored stems through its recognition
rules. `FORBIDDENWORD`, `NEEDAFFIX`, and `ONLYINCOMPOUND` pseudo-stems are not
offered because the dictionary rejects them as standalone words. `NOSUGGEST`
is retained as an explicit suggestion policy: it remains recognized when no
other rule rejects it, but it is never returned as a suggestion. The policy is
preserved in the versioned runtime artifact.

Callers may supply explicit `ReplacementRule` values for known misspellings or
organization-specific conventions. A rule that transforms the query directly
into an enumerated candidate gives that candidate ranking distance zero; it
does not add unverified words, alter `Dictionary::contains`, or emulate a
third-party suggestion order.

`HunspellDictionary::replacement_rules` exposes supported counted `REP`
entries in their source order. The CLI automatically applies those rules for
`ferrolex suggest --hunspell …`, including when the dictionary was loaded from
its provenance-bound runtime cache. A `REP` rule is two non-empty,
whitespace-free literal spellings; unsupported variants produce a warning and
never affect word recognition.

Hunspell `KEY` and counted `MAP` directives are retained as ranking signals.
For an otherwise single-character substitution, an adjacent `KEY` neighbor or
two characters from one `MAP` group receives a better ranking distance. The
reported edit distance remains the actual OSA or `REP` distance, recognition is
unchanged, and `|` separates keyboard rows. Invalid suggestion-only entries
emit warnings without preventing dictionary import.

All work is bounded by explicit candidate, word-length, and edit-cell limits.
The edit-cell budget is charged only when the candidate can reach the
dynamic-programming calculation: candidates whose length difference already
exceeds `max_edit_distance` are rejected without consuming it. Replacement-rule
matches likewise do not use edit-distance cells. The result reports whether it
is complete or stopped by a configured budget, so partial output is never
presented as exhaustive.

Suggestions are ordered by `(ranking distance, actual distance, byte-lexical
display spelling)`. If two source candidates render to the same requested
display spelling, only the first result in that order is returned, even when
other ranked entries fall between the duplicates. Case is used only for display:
comparison lowercases Unicode scalar values deterministically, while non-empty
input preserves lower, title, or upper style requested by the query. An empty
query does not imply an uppercase display style.

For repeated requests, use `Suggester::suggest_into` with a retained output
`Vec<Suggestion>` and `SuggestScratch`. The convenience `suggest` method owns
those buffers for one call; the buffer API reuses character, transformation,
dynamic-programming, and presentation workspaces without changing results.

When a candidate source carries frequency metadata, higher frequency breaks a
tie after both distances and before lexical spelling. Sources without frequency
metadata use the exact same ordering as before.

## Quality corpus and regression baseline

`ferrolex-suggest` has a deliberately small, deterministic quality corpus at
`crates/ferrolex-suggest/tests/data/suggestion-quality-corpus.tsv`. It measures
top-1 and top-3 recovery by locale and context. The integration test prints the
per-group scorecard, and CI compares it to the tracked
`suggestion-quality-baseline.tsv`; a material regression fails. Changing an
expected score is therefore an explicit baseline change with a rationale, not
a regenerated golden file.

The corpus uses original, isolated typo/target pairs only. Each record carries
its locale, context, provenance, disposition, review status, reviewer, and
review date; excluded records also carry their rationale. Entries remain marked
`requires-maintainer-review` until a maintainer approves their use in review
and records that approval.

The dedicated CI lane requires every corpus and baseline row to be
`approved-by-maintainer`, with a non-empty maintainer identity and an ISO
`YYYY-MM-DD` review date. Pending records deliberately fail that lane; an
implementation agent must never change their status on a maintainer's behalf.
The corpus policy lives beside the data in
`crates/ferrolex-suggest/tests/data/README.md`: do not copy from dictionaries,
spell-checkers, search logs, or user text without a documented compatible
license and review decision. Prefer minimal original pairs and never add an
enumerable lexicon, personal data, or long text.

The only frequency fixture deliberately creates an equal-distance tie. Its
frequency-aware score is reported separately from the frequency-free score, so
the corpus verifies that optional metadata can improve ranking without making a
frequency corpus a runtime requirement. The test data is excluded from the
published `ferrolex-suggest` package and is never read by the library at
runtime; ferrolex does not distribute a dictionary through this quality suite.
The frequency score's denominator counts only rows with a declared frequency
fixture, so a locale/context may safely contain both ordinary and
frequency-aware cases.

The `ferrolex suggest` command exposes `--max-results`,
`--max-edit-distance`, `--max-candidates`, and `--max-edit-cells`. The latter
two are useful when deliberately spending a larger, still explicit work budget
over a large installed dictionary; an incomplete result is reported on stderr,
and an empty budget-limited result includes a concrete retry hint. Repeating
`--dictionary`, `--compiled`, or `--hunspell` layers suggestion candidates from
all supplied sources in the same way as checking and analysis. The first
Hunspell-backed source supplies KEY/MAP ranking for the combined deterministic
result. OCONV presentation remains source-owned and is applied only when the
candidate belongs to that Hunspell-backed layer.
