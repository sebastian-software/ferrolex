# Compound semantics

The initial compound feature is intentionally small and deterministic.
`COMPOUNDFLAG` marks stems eligible as components, and `COMPOUNDMIN` sets the
minimum Unicode-scalar length of each component. A word is accepted as a
compound only when it can be split into exactly two eligible, non-forbidden
stems that both meet this minimum.

The check examines every UTF-8 character boundary once and performs two sorted
stem-index lookups at each candidate boundary. It considers at most 256 Unicode
scalar values; this is a bounded recognition rule, not a general asymptotic
performance claim. Longer inputs are not treated as compounds in this
compatibility level.
Compound components are exact stems: their affix-derived forms, positional
restrictions, and linking elements remain later feature gates. A
`COMPOUNDRULE` header may additionally declare up to 1,024 two-flag literal
patterns such as `AB`; the first component must carry `A` and the second `B`.
Quantifiers and rules with other component counts are rejected in strict mode
rather than being interpreted approximately. Runtime caches encode this rule
subset and reject caches produced by earlier semantics versions.
