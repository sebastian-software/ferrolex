# Hunspell import contract

This document describes the ferrolex import boundary for Hunspell-style `.aff`
and `.dic` inputs. It defines behavior, not the behavior of any other
implementation.

## Input and diagnostics

[`import`] receives already-decoded Rust strings. [`import_bytes`] reads an
ASCII `SET` declaration from the affix bytes and decodes both files as UTF-8,
ISO-8859-1, or ISO-8859-2. A missing `SET` uses the established UTF-8 default;
a UTF-8 BOM before `SET` is ignored. UTF-8 is decoded without replacement, and
the two ISO encodings use their defined one-byte mappings. An unsupported
declared encoding or malformed UTF-8 becomes a source-aware error diagnostic.

[`import_bytes_with_encodings`] accepts independently reviewed affix and
dictionary encodings for exceptional mixed pairs. Its affix encoding must still
match a present `SET` declaration. All public paths retain decoded UTF-8 text
inside the runtime dictionary and record a source name and one-based line
number for every diagnostic.

Parsing supports blank lines and lines beginning with `#`. A directive is a
whitespace-separated keyword followed by its arguments. Unknown directives are
reported with their source location and directive name.

Strict mode rejects an input that produces an error diagnostic. Lenient mode
returns the supported subset and all diagnostics. A construct whose omission
could silently accept words is an error; a suggestion-only unsupported
directive is a warning.

The public importer treats files as untrusted input. Its initial fixed limits
are 32 MiB for `.aff`, 64 MiB for `.dic`, 16 KiB per line, 100,000 parsed
affix rules, 1,000,000 parsed dictionary entries, 256 flags per entry, and 256
condition atoms. The byte-oriented entry points reject an oversized source
before scanning its `SET` declaration or decoding it. Exceeding any limit
reports an error and discards the affected input or entry; later configuration
can make these limits explicit per caller.

## Dictionary entries

The first non-comment `.dic` line may contain an entry count. The count is
validated when present but does not change recognition semantics. Each remaining
entry has this shape:

```text
stem[/flags][ <morphology fields>]
```

The importer retains the UTF-8 stem and the decoded flag set. Morphology fields
are diagnostic metadata in the initial implementation. A malformed flag section
is an error for that entry, not a panic.

## Initial AFF subset

The current compatibility level recognizes `SET`, `FLAG`, `PFX`, `SFX`,
`CIRCUMFIX`, `FORBIDDENWORD`, `NEEDAFFIX`, `KEEPCASE`, `COMPOUNDFLAG`, and
`COMPOUNDMIN`, `COMPOUNDBEGIN`, `COMPOUNDMIDDLE`, `COMPOUNDEND`,
`ONLYINCOMPOUND`, `COMPOUNDPERMITFLAG`, bounded literal `COMPOUNDRULE` patterns, and bounded
literal `BREAK` characters.
`PFX`/`SFX` headers declare a flag, whether rules cross-product, and a rule
count. Each following rule belongs to that header and has:

```text
PFX flag strip add condition
SFX flag strip add condition
```

`0` represents an empty strip, add, or condition as appropriate. An add field
can name continuation flags after `/`; those flags remain active on the
generated form under the [affix semantics](affix-semantics.md). Extra affix
morphology fields are reported and ignored. Conditions are anchored prefix or
suffix tests, according to the rule kind. See [compound semantics](compound-semantics.md)
for the deliberately bounded compound subset.

`AF`, aliases, complex prefix modes, capitalization fallbacks, compound
compound quantifiers and suggestion directives beyond the
documented subset have their own later feature gates. They must be diagnosed
rather than silently interpreted as simple affixes.

`BREAK` currently accepts a counted list of one literal Unicode-scalar
characters. A recognized word containing one of those characters is accepted
when every non-empty component is independently recognized. Anchored and
multi-scalar break patterns are strict errors rather than approximated.
