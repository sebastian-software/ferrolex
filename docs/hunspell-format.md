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

The current compatibility level recognizes `SET`, `FLAG`, `AF`, `AM`, `ICONV`,
`IGNORE`, `PFX`, `SFX`,
`CIRCUMFIX`, `FORBIDDENWORD`, `NEEDAFFIX`, `KEEPCASE`, `COMPOUNDFLAG`, and
`COMPOUNDMIN`, `COMPOUNDBEGIN`, `COMPOUNDMIDDLE`, `COMPOUNDEND`,
`ONLYINCOMPOUND`, `COMPOUNDPERMITFLAG`, bounded literal `COMPOUNDRULE` patterns, and bounded
literal `BREAK` characters, and `CHECKSHARPS`.
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

`AF` is a counted flag-alias table. A dictionary entry with a numeric flag
section, such as `word/2`, resolves to the second `AF` row before recognition;
malformed aliases never cause a later row to shift position. `AM` is a counted
morphology-alias table. Its references are validated, but their morphology text
is discarded because the current runtime does not expose morphology metadata.
Malformed `AF` is an error because it can change recognition; malformed `AM`
is a warning because it cannot. `FLAG UTF-8`/`UTF8` uses one Unicode scalar per
flag, `FLAG long` uses two scalars, and `FLAG num` uses positive comma-separated
decimal values. The selected mode applies consistently to `AF`, dictionary
entries, affix continuation flags, compound flags, and the runtime cache.

`ICONV` is a counted input-conversion table. ferrolex applies its literal
rules in source order to every lookup input before dictionary recognition; a
source ending in `_` is restricted to the end of the input word. `IGNORE`
declares Unicode scalar values removed from input, dictionary stems, and affix
strip/add text. Both directives can change recognition, so malformed input is
an error. An ignored character inside an affix condition remains an explicit
error rather than receiving guessed condition semantics. The importer limits
`ICONV` to 4,096 rules. An `ICONV` target of `0` represents the empty string.

`AF`, aliases, complex prefix modes, capitalization fallbacks, compound
compound quantifiers and suggestion directives beyond the
documented subset have their own later feature gates. They must be diagnosed
rather than silently interpreted as simple affixes.

`BREAK` currently accepts a counted list of one literal Unicode-scalar
characters. A recognized word containing one of those characters is accepted
when every non-empty component is independently recognized. Anchored and
multi-scalar break patterns are strict errors rather than approximated.

With `CHECKSHARPS`, a `KEEPCASE` stem containing `ß` additionally recognizes
its all-uppercase spelling with `SS`. No other case fallback is implied, and
an uppercase-sharp-S spelling is not accepted as that variant.

`WORDCHARS` is retained as immutable tokenization metadata and is available
from `HunspellDictionary::word_characters`. It does not alter
`Dictionary::contains`, whose argument is already one caller-segmented word;
the generic source analyzer keeps its own explicit tokenizer policy.

Counted `REP` blocks are retained for suggestion ranking. A header has the
form `REP count`, followed by exactly `count` lines shaped as `REP from to`.
Both spellings are non-empty literal whitespace-delimited strings. A leading
`^` and trailing `$` on the source spelling anchor it to the corresponding word
boundary for ranking. They do not
alter recognition; malformed `REP` input is a warning rather than an attempted
approximation. The importer retains at most 4,096 rules.
