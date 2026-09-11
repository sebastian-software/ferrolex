# ferrolex-dictionaries

Verified acquisition of reviewed third-party Hunspell dictionaries for [ferrolex](https://github.com/sebastian-software/ferrolex).

The crate does not bundle dictionary data or update it silently. Select a
catalog entry, choose the cache root, and install its digest-verified files.

```rust,no_run
use ferrolex_dictionaries::{find_locale, DictionaryInstaller, UreqFetcher};

let source = find_locale("de_DE").expect("reviewed locale");
let manifest = source.manifest()?;
let installed = DictionaryInstaller::new(UreqFetcher)
    .install(&manifest, std::path::Path::new(".ferrolex-cache"))?;
println!("{}", installed.aff_path().display());
# Ok::<(), Box<dyn std::error::Error>>(())
```

The caller owns cache placement and decides when acquisition occurs. See the
[API documentation](https://docs.rs/ferrolex-dictionaries) and the
[dictionary workflow](https://github.com/sebastian-software/ferrolex/blob/main/docs/dictionary-fetching.md).

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
