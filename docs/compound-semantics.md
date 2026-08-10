# Compound semantics

The initial compound feature is intentionally small and deterministic.
`COMPOUNDFLAG` marks stems eligible as components, and `COMPOUNDMIN` sets the
minimum Unicode-scalar length of each component. A word is accepted as a
compound only when it can be split into at least two eligible, non-forbidden
stems that all meet this minimum.

The check uses bounded dynamic segmentation over UTF-8 character boundaries
and sorted stem-index lookups. It considers at most 256 Unicode scalar values;
this is a bounded recognition rule, not a general asymptotic performance
claim. Longer inputs are not treated as compounds in this compatibility level.
Compound components are exact stems unless an explicitly permitted affix
derives a positioned component; linking elements beyond those affix rules
remain a later feature gate.
`COMPOUNDBEGIN`, `COMPOUNDMIDDLE`, and `COMPOUNDEND` can instead mark exact
stems for their respective positions. A two-component compound needs a begin
and an end stem; longer compounds use zero or more middle stems. A stem with
`ONLYINCOMPOUND` is rejected alone but may participate in an otherwise valid
compound. A
`COMPOUNDPERMITFLAG` on an affix rule's continuation flags also permits that
affix inside a compound: prefixes are otherwise limited to initial components,
suffixes to final components, and neither is allowed in a middle component.
A permit-affix remains valid as an ordinary affix outside a compound. A
`COMPOUNDRULE` header may additionally declare up to 1,024 literal patterns
with two through sixteen single-scalar flags, such as `AB` or `XYZ`; components
must carry the corresponding flags in order. Quantifiers and patterns outside
those bounds are rejected in strict mode rather than being interpreted
approximately. Runtime caches encode this rule subset and reject caches
produced by earlier semantics versions.
