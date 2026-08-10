# Hunspell import contract

This document describes the ferrolex import boundary for UTF-8 Hunspell-style
`.aff` and `.dic` inputs. It defines behavior, not the behavior of any other
implementation.

## Input and diagnostics

The importer receives the two source texts independently and records a source
name and one-based line number for every diagnostic. It never exposes legacy
encoded bytes to the runtime dictionary: a declared legacy `SET` encoding is
decoded during import or reported as unsupported.

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
condition atoms. Exceeding a limit reports an error and discards the affected
input or entry; later configuration can make these limits explicit per caller.

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
`COMPOUNDMIN`.
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
for the deliberately bounded `COMPOUNDFLAG`/`COMPOUNDMIN` subset.

`AF`, aliases, complex prefix modes, capitalization controls, compounds,
forbidden words, and suggestion directives have their own later feature gates.
They must be diagnosed rather than silently interpreted as simple affixes.
