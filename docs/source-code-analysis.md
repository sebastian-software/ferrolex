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
returned independently (`O`, `Auth`). Suggestions can later use the retained
whole-token range and segment index to reconstruct identifiers case-preservingly.

## Classification and ignores

The generic classifier identifies natural words, identifiers, acronyms, URLs,
email addresses, numbers, hexadecimal hashes, local paths, and long
Base64-shaped ASCII data. URLs, email addresses, numbers, hashes, paths, and
Base64-shaped data are ignored by default but remain configurable categories.
The path and Base64 heuristics are deliberately conservative: neither is a
claim to parse every platform path or binary encoding. Exact ignored words and
regular expressions are supported; an ignore expression must match the
complete raw token, not merely a substring.

Project and user vocabulary belongs in a layered `Dictionary`. `UserDictionary`
is the mutable overlay: adding a word is immediately visible to concurrent
lookups without mutating base dictionaries. Its `from_text` and `to_text`
methods use the same deterministic UTF-8 word-list syntax as base lists, so a
caller can atomically persist an overlay without imposing filesystem policy on
the core crate.

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
