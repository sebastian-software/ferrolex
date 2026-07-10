# ferrolex

`ferrolex` is a Rust-native spell, dictionary, and Lexios brand validation
toolkit for code and localization workflows.

The project starts from a narrow premise: codebases and translation catalogs do
not contain only prose. They contain identifiers, source strings, ICU messages,
JSON values, product names, acronyms, and brand terminology. Ferrolex aims to
extract candidate text, split it into checkable tokens, validate it against
language dictionaries, and apply Lexios-backed brand rules without treating
programming-language syntax as copy.

The repository is currently a workspace skeleton. The first implementation work
will focus on tokenization, dictionary import, and the validation pipeline.

## Direction

- Rust first, with Node bindings deferred until the Rust API is stable enough.
- Hunspell-compatible dictionaries as source material, compiled into a
  Ferrolex-owned runtime format.
- Lexios as the canonical brand model. Ferrolex should not grow a competing
  brand guide schema.
- JSON and OXC adapters first for source candidate extraction.
- No Tree-sitter, CodeSitter, or Palamedes-specific extraction rules in v0.1.

## Workspace

- `ferrolex`: umbrella crate and recommended public dependency.
- `ferrolex-core`: spans, tokens, findings, severities, locales, and traits.
- `ferrolex-tokenizer`: Unicode tokenization and code identifier splitting.
- `ferrolex-dict`: dictionary metadata and runtime lookup contracts.
- `ferrolex-hunspell`: Hunspell fetch, import, and conversion layer.
- `ferrolex-brand`: Lexios-powered brand validation runtime.
- `ferrolex-json`: JSON candidate extraction.
- `ferrolex-oxc`: JavaScript and TypeScript candidate extraction via OXC.

## Planned Pipeline

1. Source adapters extract inspectable spans from JSON, JavaScript, TypeScript,
   localization files, or direct caller input.
2. The tokenizer splits natural-language text and code identifiers into
   checkable token candidates.
3. Dictionary providers validate token membership and produce spelling
   suggestions.
4. Lexios-derived brand rules validate canonical spellings, aliases, forbidden
   variants, casing, and translation guidance.
5. Checkers emit structured findings that can be rendered by CLIs, CI systems,
   editors, or downstream tools.

## Dictionary Strategy

Hunspell is the practical v0.1 dictionary source because the ecosystem has broad
language coverage and mature `.aff` / `.dic` dictionaries. Ferrolex should use
Hunspell as import material, not as the runtime boundary.

The intended flow is:

1. Fetch approved dictionary sources.
2. Preserve source, version, and per-language license metadata.
3. Import `.aff` and `.dic` files.
4. Compile the result into a Ferrolex-owned runtime format.
5. Check words through Ferrolex's dictionary API.

See [docs/dictionaries.md](docs/dictionaries.md).

## Lexios Brand Validation

Brand validation belongs to Lexios. `ferrolex-brand` consumes Lexios Brand ID
data and turns it into runtime checks. If Ferrolex needs better brand semantics,
those changes should be proposed or implemented in Lexios first.

See [docs/brand-validation.md](docs/brand-validation.md).

## Development

```sh
cargo check --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

<!-- ferramenta-family:start -->
## The Ferramenta family

This project is part of [Ferramenta](https://ferramenta.dev) — the family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com) that keep the APIs the ecosystem already knows:

| Tool | Job |
| --- | --- |
| [ferroni](https://github.com/sebastian-software/ferroni) | Oniguruma-compatible regex engine |
| [ferriki](https://github.com/sebastian-software/ferriki) | Shiki-compatible syntax highlighting |
| [ferromark](https://github.com/sebastian-software/ferromark) | CommonMark/GFM Markdown to HTML |
| [ferrovia](https://github.com/sebastian-software/ferrovia) | SVGO-compatible SVG optimizer |
| [ferrocat](https://github.com/sebastian-software/ferrocat) | Translation catalog engine |
| **[ferrolex](https://github.com/sebastian-software/ferrolex)** | Spell, dictionary, and brand validation |
| [ferrugo](https://github.com/sebastian-software/ferrugo) | Rust-native PDF previews |

ferrocat and ferrolex are also the Rust foundation of [Palamedes](https://github.com/sebastian-software/palamedes), i18n tooling for JavaScript and TypeScript teams that want one translation model to survive framework changes.
<!-- ferramenta-family:end -->
