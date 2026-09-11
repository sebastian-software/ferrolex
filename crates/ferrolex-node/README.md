# `@ferrolex/node`

Native Node.js bindings for the ferrolex spell-checking engine.

```js
const { SpellChecker } = require('@ferrolex/node')

const checker = new SpellChecker('ferrolex\nFerris')
checker.check('ferrolex')
checker.suggest('ferolex')
```

The package also supports strict caller-owned Hunspell files with
`SpellChecker.fromHunspell(affPath, dicPath)` and digest-pinned managed
dictionaries with `await SpellChecker.install(locale, cacheRoot)`. It can also
load a validated standalone runtime artifact with
`SpellChecker.fromRuntimeArtifact(path)` or
`SpellChecker.fromRuntimeArtifactBytes(buffer)`. Dictionary data is never
bundled; callers always select its source files or cache directory.

Supported prebuilt targets cover Linux x64/arm64 glibc and musl, macOS arm64
and x64, and Windows x64 and arm64. The release workflow builds the musl
variants without runtime execution on a glibc runner.
Node.js 22.13 or newer is required.

See the [complete binding documentation][bindings] in the source repository.

[bindings]: https://github.com/sebastian-software/ferrolex/blob/main/docs/bindings.md

<!-- ferramenta-family:start -->
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — A family of Rust tools.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) — Oniguruma-compatible regex engine · [ferriki](https://github.com/sebastian-software/ferriki) — Shiki-compatible syntax highlighting · [ferromark](https://sebastian-software.github.io/ferromark/) — Markdown to HTML with a secure default and every GFM extension included. · [ferrocat](https://ferrocat.dev) — Translation catalog engine · [palamedes](https://palamedes.dev) — Internationalization for TypeScript applications · [ferrovia](https://github.com/sebastian-software/ferrovia) — SVGO-compatible SVG optimizer · [ferralk](https://github.com/sebastian-software/ferralk) — Glob matching and parallel filesystem walking · [ferrugo](https://github.com/sebastian-software/ferrugo) — PDF previews for untrusted files.
<!-- ferramenta-family:end -->
