# Compound semantics

The initial compound feature is intentionally small and deterministic.
`COMPOUNDFLAG` marks stems eligible as components, and `COMPOUNDMIN` sets the
minimum Unicode-scalar length of each component. A word is accepted as a
compound only when it can be split into exactly two eligible, non-forbidden
stems that both meet this minimum.

The check tries every UTF-8 character boundary once, so it is linear in the
input length. It considers at most 256 Unicode scalar values. Longer inputs
are not treated as compounds in this compatibility level. Compound components
are exact stems: their affix-derived forms, positional restrictions, linking
elements, and `COMPOUNDRULE` patterns remain later feature gates.
