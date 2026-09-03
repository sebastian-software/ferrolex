# `@ferrolex/node`

Native Node.js bindings for the ferrolex spell-checking engine.

```js
const { Checker } = require('@ferrolex/node')

const checker = new Checker('ferrolex\nFerris')
checker.check('ferrolex')
checker.suggest('ferolex')
```

The package also supports strict caller-owned Hunspell files with
`Checker.fromHunspell(affPath, dicPath)` and digest-pinned managed dictionaries
with `await Checker.install(locale, cacheRoot)`. Dictionary data is never
bundled; callers always select its source files or cache directory.

Supported prebuilt targets are Linux x64 glibc, macOS arm64, and Windows x64.
Node.js 22.13 or newer is required.

See the [complete binding documentation][bindings] in the source repository.

[bindings]: https://github.com/sebastian-software/ferrolex/blob/main/docs/bindings.md
