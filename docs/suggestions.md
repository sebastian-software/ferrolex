# Suggestions

`ferrolex-suggest` is deliberately separate from `Dictionary::contains`.
Candidate sources enumerate stable lexical candidates; a generic dictionary is
not assumed to be enumerable. The first implementation uses Unicode-scalar
optimal-string-alignment distance, including adjacent transpositions, and
ranks by `(distance, byte-lexical spelling)`.

All work is bounded by explicit candidate, word-length, and edit-cell limits.
The result reports whether it is complete or stopped by a configured budget, so
partial output is never presented as exhaustive. Case is used only for display:
comparison lowercases Unicode scalar values deterministically, while output
preserves lower, title, or upper style requested by the query.
