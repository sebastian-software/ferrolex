# Suggestions

`ferrolex-suggest` is deliberately separate from `Dictionary::contains`.
Candidate sources enumerate stable lexical candidates; a generic dictionary is
not assumed to be enumerable. The first implementation uses Unicode-scalar
optimal-string-alignment distance, including adjacent transpositions, and
ranks by `(distance, byte-lexical spelling)`.

`WordList`, `UserDictionary`, and `HunspellDictionary` are candidate sources.
A mutable `UserDictionary` is snapshotted before suggestion work begins, so
project-word updates never hold its lock across edit-distance comparisons.
Hunspell candidates are its stored stems in UTF-8 byte order; affix-derived and
compound forms are deliberately not enumerated because a source can describe
an unbounded form space.

Callers may supply explicit `ReplacementRule` values for known misspellings or
organization-specific conventions. A rule that transforms the query directly
into an enumerated candidate gives that candidate ranking distance zero; it
does not add unverified words, alter `Dictionary::contains`, or emulate a
third-party suggestion order.

All work is bounded by explicit candidate, word-length, and edit-cell limits.
The result reports whether it is complete or stopped by a configured budget, so
partial output is never presented as exhaustive. Case is used only for display:
comparison lowercases Unicode scalar values deterministically, while output
preserves lower, title, or upper style requested by the query.

The `ferrolex suggest` command exposes `--max-results`,
`--max-edit-distance`, `--max-candidates`, and `--max-edit-cells`. The latter
two are useful when deliberately spending a larger, still explicit work budget
over a large installed dictionary; an incomplete result is reported on stderr.
