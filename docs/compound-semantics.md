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
must carry the corresponding flags in order. Parenthesized groups are diagnosed
as unsupported rather than interpreted as literal flags. For Unicode flag mode,
postfix `*`, `+`, and `?` expand during import into only the possible
two-through-sixteen component sequences. Expansion is capped at 1,024 patterns
per rule and 16,384 per dictionary, with an import diagnostic on overflow.
Runtime matching therefore keeps its existing bounded segmentation model.
Runtime caches encode the expanded rule subset and reject caches produced by
earlier semantics versions.

## Compound safeguards

The compound matcher applies safeguards to the same candidate segmentation; it
does not first accept a compound and then try to repair it. `COMPOUNDFORBIDFLAG`
removes a flagged direct stem from beginning and middle compound positions, and
removes a flagged derived form from every compound position, even when it is
otherwise eligible through `COMPOUNDFLAG` or `COMPOUNDPERMITFLAG`.

`COMPOUNDWORDMAX` caps the number of components. When `COMPOUNDSYLLABLE` is
also declared, a compound above that cap is permitted only when its Unicode
scalar vowel count does not exceed the declared maximum. The vowel set is read
literally from the AFF declaration. Both settings are bounded by the existing
256-scalar compound-query limit.

`CHECKCOMPOUNDDUP` rejects adjacent equal components. `CHECKCOMPOUNDCASE`
rejects an uppercase first scalar after a component boundary, and
`CHECKCOMPOUNDTRIPLE` rejects a triple equal scalar that crosses one.
`SIMPLIFIEDTRIPLE` additionally permits the two-scalar spelling obtained by
removing one scalar from that boundary triple. `FORCEUCASE` requires an
uppercase first scalar of the whole compound when its final component has the
configured flag.

`CHECKCOMPOUNDREP` rejects a compound when a declared `REP` typo replacement
turns it into an otherwise accepted stored word or one-affix form. A
`CHECKCOMPOUNDPATTERN` rule rejects a component boundary whose left component
ends and right component begins with the declared strings; optional flag
conditions apply to those selected components. A replacement form is evaluated
as a single bounded virtual spelling of that boundary, never by recursive
compound recognition. At most 32 matching replacement locations are examined
per declared pattern. These guards retain the 256-scalar input bound and use
only a finite set of split positions and declared pattern/replacement variants.
