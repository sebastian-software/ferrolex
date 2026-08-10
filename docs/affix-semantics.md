# Affix semantics

The initial morphology model is independent of the textual Hunspell syntax.
A lexeme contains a stem and capabilities. A prefix or suffix rule transforms a
lexeme only when its capability flag is present and its condition matches.

## Rule application

For a prefix rule, ferrolex removes the declared prefix from the start of the
stem, verifies the condition against the remaining stem, and prepends the add
text. For a suffix rule it performs the analogous operation at the end. A
failed strip or condition rejects that rule; it is not an exceptional state.

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

The initial milestone does not recursively apply continuation classes, and it
does not promise circumfix, need-affix, forbidden-word, or compound semantics.
Those features are additive gates rather than implicit side effects of the
basic rule evaluator.
