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
A single permit-affix transformation is matched by inverse rule application
against the stem index; multi-step permit chains remain a later feature gate.
A permit-affix remains valid as an ordinary affix outside a compound. A
`COMPOUNDRULE` header may additionally declare up to 1,024 literal patterns
with two through sixteen component flags, such as `AB` or `XYZ`; components
must carry the corresponding flags in order. For Unicode flag mode, postfix
`*`, `+`, and `?` expand during import into only the possible two-through-sixteen
component sequences. Runtime matching therefore keeps its existing bounded
segmentation model. Runtime caches encode the expanded rule subset and reject
caches produced by earlier semantics versions.
