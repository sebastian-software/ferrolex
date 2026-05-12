# Brand Validation

Ferrolex brand validation is Lexios-based. There should not be a separate
Ferrolex brand schema that competes with Lexios.

## Source of Truth

Lexios Brand ID data is the canonical model for brand knowledge. Ferrolex should
consume Lexios and compile the relevant parts into runtime checks.

The first fields of interest are:

- `profile.lexicon.entries[].term`
- `profile.lexicon.entries[].canonicalSpelling`
- `profile.lexicon.entries[].aliases`
- `profile.lexicon.entries[].forbiddenVariants`
- `profile.lexicon.entries[].casingRule`
- `profile.lexicon.entries[].translationPolicy`
- `profile.lexicon.entries[].translationNotes`

If Ferrolex needs richer terminology, casing, translation, or style semantics,
those concepts should be proposed or implemented in Lexios first.

## Runtime Responsibility

`ferrolex-brand` should be responsible for:

- Loading or receiving validated Lexios Brand ID data.
- Compiling Lexios lexicon entries into efficient runtime checks.
- Checking accepted terms, aliases, canonical spelling, forbidden variants, and
  casing.
- Emitting standard Ferrolex findings.

`ferrolex-brand` should not be responsible for inventing a parallel brand guide
format.

## Schema Follow-Up For Lexios

Lexios currently exposes TypeScript/Zod types and YAML helpers. Rust consumption
would be cleaner if Lexios also emitted a stable language-neutral schema
artifact, such as JSON Schema.

That work belongs in the Lexios repository. Ferrolex can start with a narrow
projection for the fields it needs, but the long-term contract should be driven
by Lexios.

## Rule Semantics

Brand checks should start deterministic:

- canonical spelling checks
- forbidden variant checks
- casing checks
- locale-specific translation notes
- translation policy warnings

Tone and voice checks can be introduced later, but they should still originate
from Lexios concepts instead of Ferrolex-local brand vocabulary.
