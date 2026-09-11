# ferrolex-hunspell

Hunspell-compatible dictionary import for [ferrolex](https://github.com/sebastian-software/ferrolex).

The importer accepts the documented supported subset of textual Hunspell AFF
and DIC files, reports source-aware diagnostics, and produces a dictionary with
recognition and suggestion metadata.

```rust
use ferrolex_core::Dictionary;
use ferrolex_hunspell::{import, ImportMode};

let imported = import(
    "example.aff",
    "SET UTF-8\n",
    "example.dic",
    "1\nferrolex\n",
    ImportMode::Strict,
)?;
assert!(imported.dictionary().contains("ferrolex"));
# Ok::<(), ferrolex_hunspell::ImportError>(())
```

See the [API documentation](https://docs.rs/ferrolex-hunspell), the
[Hunspell format guide](https://github.com/sebastian-software/ferrolex/blob/main/docs/hunspell-format.md),
and the [compatibility notes](https://github.com/sebastian-software/ferrolex/blob/main/docs/locale-compatibility.md).

<!-- ferramenta-family:start -->
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — A family of Rust tools.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) — Oniguruma-compatible regex engine · [ferriki](https://github.com/sebastian-software/ferriki) — Shiki-compatible syntax highlighting · [ferromark](https://sebastian-software.github.io/ferromark/) — Markdown to HTML with a secure default and every GFM extension included. · [ferrocat](https://ferrocat.dev) — Translation catalog engine · [palamedes](https://palamedes.dev) — Internationalization for TypeScript applications · [ferrovia](https://github.com/sebastian-software/ferrovia) — SVGO-compatible SVG optimizer · [ferralk](https://github.com/sebastian-software/ferralk) — Glob matching and parallel filesystem walking · [ferrugo](https://github.com/sebastian-software/ferrugo) — PDF previews for untrusted files.
<!-- ferramenta-family:end -->

<!-- sebastian-software-branding:start -->

<p align="center">
  <a href="https://oss.sebastian-software.com"><img src="https://sebastian-brand.vercel.app/sebastian-software/logo-software.svg" alt="Sebastian Software" width="160" /></a><br />
  TypeScript, React &amp; Rust consulting<br />
  Experts in Agentic Software Development<br />
  <a href="https://sebastian-software.de">Work with us</a> · <a href="https://oss.sebastian-software.com">More open source</a>
</p>

<p align="center">Copyright &copy; 2026 Sebastian Software GmbH</p>

<!-- sebastian-software-branding:end -->
