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
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — Rust-native developer tools that keep the APIs the ecosystem already knows.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) · [ferriki](https://github.com/sebastian-software/ferriki) · [ferromark](https://sebastian-software.github.io/ferromark/) · [ferrocat](https://ferrocat.dev) · [palamedes](https://palamedes.dev) · [ferrovia](https://github.com/sebastian-software/ferrovia) · [ferralk](https://github.com/sebastian-software/ferralk) · [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:end -->
