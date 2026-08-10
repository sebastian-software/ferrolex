# Suggestions

`ferrolex-suggest` is deliberately separate from `Dictionary::contains`.
Candidate sources enumerate stable lexical candidates; a generic dictionary is
not assumed to be enumerable. The first implementation uses Unicode-scalar
optimal-string-alignment distance, including adjacent transpositions, and
ranks by `(distance, byte-lexical spelling)`.

`WordList` and `UserDictionary` are candidate sources. A mutable
`UserDictionary` is snapshotted before suggestion work begins, so project-word
updates never hold its lock across edit-distance comparisons.

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
