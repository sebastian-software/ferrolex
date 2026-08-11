# Affix semantics

The initial morphology model is independent of the textual Hunspell syntax.
A lexeme contains a stem and capabilities. A prefix or suffix rule transforms a
lexeme only when its capability flag is present and its condition matches.

## Rule application

For a prefix rule, ferrolex verifies the condition against the start of the
original stem, removes the declared prefix, and prepends the add text. For a
suffix rule it verifies the condition against the end of the original stem,
removes the declared suffix, and appends the add text. A failed strip or
condition rejects that rule; it is not an exceptional state.

The original stem remains recognized unless a later rule explicitly changes
that behavior. Generated forms are evaluated lazily during lookup rather than
expanded into the base word list.

## Conditions

The initial condition language supports:

- `.` for one Unicode scalar value;
- a literal character;
- a bracket class such as `[abc]`;
- a negated bracket class such as `[^abc]`.
- a bounded negative one-scalar lookbehind such as `(?<!і)[з]вати`;
- `(^|[^о])…` for an initial stem or a one-scalar negative predecessor; and
- a start-anchored literal or bracket pattern such as `(^весь)`.

Prefix conditions match at the start and suffix conditions at the end. A
condition is not a general regular expression: unsupported syntax produces a
diagnostic. The supported lookbehind and start-anchor forms are normalized into
bounded character checks before lookup.

## Cross-product

Rules whose headers opt into cross-product may be composed as one prefix and up
to two suffixes when the lexeme carries the required flags. The prefix operates
on the stem first, then the suffixes operate on the prefixed form. Rules without
that opt-in are never combined merely because both independently match.

## Advanced flags

Continuation flags following an add field (`add/flags`) are the only same-kind
capabilities retained by the derived form and can unlock a second suffix rule.
A prefix-to-suffix transition additionally requires cross-product opt-in on
both rules and uses the original lexeme capabilities. A derivation may apply a
particular rule only once, one prefix and two suffixes at most, is limited to
eight transformations, and examines at most 4,096 derived states per lexeme.
Lookup also stops reverse-affix candidate collection after 8,192 lexemes.
Budget exhaustion deterministically rejects the lookup. Dictionaries requiring
deeper or more branching chains are outside the current compatibility level and
must not be treated as equivalent.

`CIRCUMFIX` is represented by a flag in continuation fields. A form created
through a circumfix-marked prefix is accepted only after a circumfix-marked
suffix is also applied, and conversely. `NEEDAFFIX` rejects a stem or derived
form while it remains in that form's continuation flags. `ONLYINCOMPOUND` on a
lexeme or continuation likewise rejects an ordinary derived form but permits it
as an eligible compound component. `FORBIDDENWORD` rejects the complete lexeme
and all forms derived from it, taking precedence over every positive rule.

Recognition first checks exact UTF-8 matching. Initial-capital and all-uppercase
input additionally receives the Hunspell capitalization fallback even without a
`LANG` directive: lower- and initial-case candidates are checked without
admitting `KEEPCASE` lexemes or forms derived from them. Turkish, Azeri, and
Crimean Tatar apply their dotted/dotless-I casing when selected by `LANG`; other
dictionaries use Unicode default casing. Mixed-case input remains exact.
