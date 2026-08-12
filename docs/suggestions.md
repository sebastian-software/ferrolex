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

The `ferrolex suggest` command exposes `--max-results`,
`--max-edit-distance`, `--max-candidates`, and `--max-edit-cells`. The latter
two are useful when deliberately spending a larger, still explicit work budget
over a large installed dictionary; an incomplete result is reported on stderr.
