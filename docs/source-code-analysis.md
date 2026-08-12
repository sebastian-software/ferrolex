# Source-code analysis

`ferrolex-code` is the generic, parser-independent analysis layer. It consumes
an immutable `Dictionary`, preserves the source token and its UTF-8 byte range,
and reports misspelled segments without changing dictionary semantics.

## Identifier segmentation

The default splitter separates underscores, hyphens, digits, and symbols. It
also separates lowercase-to-uppercase and acronym-to-word boundaries:

```text
userAuthenticator               -> user, Authenticator
OAuthAuthenticationProvider     -> OAuth, Authentication, Provider
HTTPResponseCode                -> HTTP, Response, Code
user_profile_image              -> user, profile, image
```

Unicode letter boundaries are respected. A caller can select whether a leading
single uppercase letter remains attached to its following word (`OAuth`) or is
returned independently (`O`, `Auth`).

`recombine_identifier_suggestion` turns a suggested replacement for an
identifier segment into one complete token edit. It retains the surrounding
identifier text and follows lower-, upper-, and initial-uppercase segment
casing, for example `OAuthAuthentcationProvider` becomes
`OAuthAuthenticationProvider`.

## Classification and ignores

The generic classifier identifies natural words, identifiers, acronyms, URLs,
email addresses, numbers, hexadecimal hashes, local paths, UUIDs, bare domains,
conventionally delimited generated tokens, and long Base64-shaped ASCII data.
These machine-shaped classes are ignored by default but remain configurable.
The path and Base64 heuristics are deliberately conservative: Base64 requires
one of `+`, `/`, or `=`, and a bare hexadecimal token needs a digit (unless it
uses `0x`), so ordinary words and camel-case identifiers remain checked. Exact
ignored words and regular expressions are supported; expressions are compiled
as full-token matches, including alternations.

Project and user vocabulary belongs in a layered `Dictionary`. `UserDictionary`
is the mutable overlay: adding a word is immediately visible to concurrent
lookups without mutating base dictionaries. Its `from_text` and `to_text`
methods use the same deterministic UTF-8 word-list syntax as base lists, so a
caller can atomically persist an overlay without imposing filesystem policy on
the core crate.

## Technical vocabulary

Use a reviewed plain UTF-8 word list for technology names, APIs, and product
terms. Pass it as an additional base dictionary; ferrolex combines dictionaries
without giving the analyzer a separate vocabulary format:

```sh
ferrolex analyze --dictionary language.txt --dictionary technical.txt src/
```

Keep the technical list separate from a project's mutable user overlay. Record
its source revision and license before importing third-party data. ferrolex does
not read `cspell.json` or cspell dictionary formats; an approved source must be
converted to the normal word-list contract during a reviewed import step.

## Persistent project policy

`ferrolex analyze --config .ferrolex/config` reads a deliberately small,
line-oriented policy file. Blank lines and `#` comments are ignored. Its
canonical entries are:

```text
ignore-word = OAuth
ignore-pattern = ^generated_[a-z]+$
minimum-word-length = 3
single-letter-prefix = separate
```

Keys are strict and values are validated with their source line. The file is
not a general TOML/YAML compatibility layer; keeping this contract small makes
the library, CLI, and future editor integrations share the same policy without
an ambient parser or silent configuration drift.

## Inline directives

Directives are only interpreted in the comment syntax supplied with a
`Document`; text that merely resembles a directive has no special meaning.

```text
// ferrolex:ignore OAuthAuthentcationProvider
// ferrolex:disable
// ferrolex:enable
```

`ignore` words apply to the complete document. `disable` applies after its
directive line until a following `enable`; the directives are intentionally not
nested. Malformed or unknown directives produce structured diagnostics and do
not prevent the rest of the document from being analyzed.

The format is ferrolex-specific. cspell directives and configuration are not
interpreted.
