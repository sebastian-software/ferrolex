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

Prefix conditions match at the start and suffix conditions at the end. A
condition is not a general regular expression: unsupported syntax produces a
diagnostic.

## Cross-product

Rules whose headers opt into cross-product may be composed once as one prefix
and one suffix when the lexeme carries both flags. The prefix operates on the
stem first, then the suffix operates on the prefixed form. Rules without that
opt-in are never combined merely because both independently match.

## Advanced flags

Continuation flags following an add field (`add/flags`) are the only same-kind
capabilities retained by the derived form and can unlock another rule. A
prefix-to-suffix (or inverse) transition additionally requires cross-product
opt-in on both rules and uses the original lexeme capabilities. A derivation
may apply a particular rule only once, is limited to eight transformations,
and examines at most 4,096 derived states per lexeme. Budget exhaustion
deterministically rejects the lookup. Dictionaries requiring deeper or more
branching chains are outside the current compatibility level and must not be
treated as equivalent.

`CIRCUMFIX` is represented by a flag in continuation fields. A form created
through a circumfix-marked prefix is accepted only after a circumfix-marked
suffix is also applied, and conversely. `NEEDAFFIX` rejects its bare stem but
allows a valid derived form. `FORBIDDENWORD` rejects the complete lexeme and
all forms derived from it, taking precedence over every positive rule.

Recognition is exact UTF-8 matching. `KEEPCASE` is imported as an explicit
marker, but has no additional transformation in this level: neither marked nor
unmarked words receive implicit lower-, upper-, or title-case fallbacks.
