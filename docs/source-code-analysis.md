# Generic token analysis

`ferrolex-code` is an existing generic, parser-independent helper. It consumes
an immutable `Dictionary`, preserves source tokens and UTF-8 byte ranges, and
reports misspelled segments without changing dictionary semantics.

It is not the product boundary for Markdown, PO, TypeScript, Rust, or another
format. Format-owning tools should parse their own input and call the ferrolex
dictionary and suggestion APIs with the selected text. ferrolex will not add
language grammars, parser dependencies, semantic analysis, or per-language
support tiers.

## Generic behavior

Generic support is available for every input, including unsupported file types.
The analyzer classifies generic tokens, segments identifiers, applies the
configured dictionary and ignore policy, and reports original UTF-8 byte
ranges. It does not parse the file, so it cannot distinguish a real comment or
string literal from text that merely looks like one in that language.

The existing `Analyzer`, `Document`, project configuration, dictionary
selection, ignore classes and patterns, directives, and identifier-suggestion
contract remain the supported fallback for unsupported languages. Adding a
language integration must not change their defaults, keys, precedence, or
meaning for generic documents.

File-extension presets select only the comment marker used for `ferrolex:`
directives. Ordinary token classification and checking remain the generic
pass. These presets do not parse a grammar, recover from syntax errors,
identify string literals, or understand embedded languages. A caller can
always override the preset with an explicit `Document` comment syntax or
CLI/configuration setting.

Parser and semantic support belong to consumer projects. See
[ADR-0009](adr/0009-language-aware-source-analysis.md) for the product-boundary
decision.

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

For one-off integrations, `Analyzer::check_identifier` applies the same
configured splitting and policy directly to an identifier. Every identifier
`Finding` can turn a segment candidate into its full-token replacement with
`whole_identifier_suggestion`; the older free recombination helper remains
available. `ferrolex analyze --suggest` emits up to three bounded suggestions
per finding and renders identifier candidates as full, case-preserving edits.

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

## Unicode normalization and prose tokens

Findings always retain the original source token and UTF-8 byte range. Before a
dictionary lookup, ferrolex also tries its NFC form; this recognizes decomposed
(NFD) text without rewriting source or silently changing a replacement range.
Combining marks remain attached to their base letters, and straight or curly
apostrophes between letters remain part of prose words such as `don't` and
`l’esprit`.

Project and user vocabulary belongs in a layered `Dictionary`. `UserDictionary`
is the mutable overlay: adding a word is immediately visible to concurrent
lookups without mutating base dictionaries. Its `from_text` and `to_text`
methods use the same deterministic UTF-8 word-list syntax as base lists, so a
caller can atomically persist an overlay without imposing filesystem policy on
the core crate.

The CLI stores workspace additions in `.ferrolex/words.txt` and global
additions in `$XDG_CONFIG_HOME/ferrolex/words.txt` (or
`$HOME/.config/ferrolex/words.txt`). Use `ferrolex dictionary add-word WORD`
for the current workspace, `--workspace PATH` for an explicit project root, or
`--global` for the user-wide list. Each update writes a sorted complete
replacement to a temporary sibling and then renames it atomically. Concurrent
processes should serialize add-word operations themselves: atomic replacement
prevents partial files but intentionally does not merge two independently read
snapshots.

## Technical vocabulary

Use a reviewed plain UTF-8 word list for technology names, APIs, and product
terms. Pass it as an additional base dictionary; ferrolex combines dictionaries
without giving the analyzer a separate vocabulary format:

```sh
ferrolex analyze --dictionary language.txt --dictionary technical.txt src/
```

`analyze` accepts either one file or a directory. Directories are traversed
recursively in deterministic path order. Use repeated `--include <GLOB>` and
`--exclude <GLOB>` options to select relative paths; `*` stays within one
directory, while `**` may cross directory boundaries. With no include glob,
every regular file is analyzed. Excludes always win. The command returns zero
only when every selected file is clean; any finding or malformed directive in
any selected file returns the ordinary misspelling exit status.

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
include = **/*.rs
exclude = target/**
```

`include` and `exclude` use the same relative path glob rules as the CLI and
combine with CLI patterns. They are selection policy only; comment syntax can
still be chosen for one invocation with the CLI hook.

Project configuration can also declare the analysis dictionary sources, relative
to the configuration file: `dictionary = words.txt`,
`compiled-dictionary = words.flex`, and `hunspell = en_US.aff`. This permits
`ferrolex analyze --config .ferrolex/config src/` without repeating a
dictionary flag. `ignore-class = url` adds a token class to the ignore policy;
`check-class = domain` removes one of the default ignores. Supported names are
`natural-word`, `identifier`, `acronym`, `url`, `email`, `number`, `hash`,
`path`, `base64`, `uuid`, `domain`, `generated-token`, and `unknown`.

Keys are strict and values are validated with their source line. The file is
not a general TOML/YAML compatibility layer. Keeping this contract small lets
the library and CLI share policy without an ambient parser or silent
configuration drift. Format-aware consumers should normally select text before
calling ferrolex instead of adopting this project configuration format.

## Inline directives

Directives are only interpreted in the comment syntax supplied with a
`Document`; text that merely resembles a directive has no special meaning.
The CLI accepts a line prefix with `--comment-prefix` (including dash prefixes
such as `--comment-prefix=--`) or HTML comments with `--comment-syntax html`.
Without an explicit option, extension presets select `//` for common C-family
files, `#` for shell/Python/config files, `--` for SQL/Lua/Haskell, and HTML
comments for Markdown, HTML, and XML. Markdown prose and fenced code remain in
the same generic, parser-independent analysis pass; a language parser is
deliberately outside this helper's scope.
Set `comment-syntax = html`, `comment-syntax = none`, or
`comment-syntax = line://` in the project configuration to override those
presets; an explicit CLI option takes precedence.

```text
// ferrolex:ignore OAuthAuthentcationProvider
// ferrolex:disable
// ferrolex:enable
```

`ignore` words apply to the complete document. `disable` applies after its
directive line until a following `enable`; the directives are intentionally not
nested. Malformed or unknown directives produce structured diagnostics and do
not prevent the rest of the document from being analyzed.

Directives must occupy a complete comment line, apart from leading whitespace.
For example, `let value = 1; // ferrolex:disable` is ordinary source text and
does not alter analysis. This keeps directive recognition parser-independent
and avoids treating comment-like text inside strings as configuration.

The format is ferrolex-specific. cspell directives and configuration are not
interpreted.
