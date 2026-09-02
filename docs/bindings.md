# Node.js binding and deferred prototypes

`@ferrolex/node` is ferrolex's supported pre-1.0 direct non-Rust integration.
The repository and release owner is `sebastian-software`. The package is
publication-ready but has not yet been published to npm; the first publication
requires verified registry access to the `@ferrolex` scope.

The package requires Node.js 22.13 or newer and uses Node-API 8. Its generated
CommonJS loader and TypeScript declarations are checked in so changes to the
JavaScript contract are reviewable.

## TypeScript API

The API deliberately mirrors the focused Rust concepts:

```ts
import { Checker, dictionaryCatalog } from '@ferrolex/node'

const words = new Checker('ferrolex\nFerris')
words.check('ferrolex')
words.suggest('ferolex')

const local = Checker.fromHunspell('dictionary.aff', 'dictionary.dic')
local.check('derived-form')

const managed = await Checker.install('en_US', '.ferrolex-dictionaries')
managed.suggest('recieve')

const sources = dictionaryCatalog()
```

`Checker.fromHunspell` strictly imports caller-owned `.aff` and `.dic` files.
`Checker.install` fetches, verifies, caches, and strictly imports a
digest-pinned catalog dictionary off the JavaScript event loop. The caller
always selects the cache root. Ferrolex neither bundles dictionary data nor
uses a global implicit download location.

`dictionaryCatalog` exposes the reviewed locale, pinned revision, SPDX license
expression, and immutable upstream license-notice URL. `suggest` uses the same
bounded, deterministic engine as Rust, including imported Hunspell replacement
and ranking signals.

## Prebuilt and runtime policy

The package publishes one optional native package for every declared target:

| Target | npm package | CI runtime |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `@ferrolex/node-linux-x64-gnu` | Node.js 22.13 and 24 |
| `aarch64-apple-darwin` | `@ferrolex/node-darwin-arm64` | Node.js 24 |
| `x86_64-pc-windows-msvc` | `@ferrolex/node-win32-x64-msvc` | Node.js 24 |

Other CPU, operating-system, and libc combinations are unsupported until they
have a named maintainer and the same build, runtime, and clean-install gates.
The root package pins every optional native package to the exact workspace
version; no consumer compiler is required.

CI builds and loads each declared native target, runs the JavaScript API tests,
checks that napi-rs regenerates the committed loader and declarations without a
diff, then installs the packed root and platform tarballs in an empty consumer
directory. Linux CI additionally exercises the managed `en_US` path against a
fresh verified cache. The release-version contract keeps the Cargo crate, npm
root package, lockfile, and platform-package pins on the same version.

For a checkout on a supported host:

```sh
npm ci --prefix crates/ferrolex-node --ignore-scripts
npm run build --prefix crates/ferrolex-node
npm test --prefix crates/ferrolex-node
bash scripts/test-node-package.sh
```

After registry publication, consumers install only the root package:

```sh
npm install @ferrolex/node
```

## Reproducible benchmark

The Node.js runtime check compares 12,000 mixed recognized/missing queries
against a native `Set` containing the same 4,097 words. It also asserts that
`ferolex` suggests `ferrolex`:

```sh
bash scripts/bench-node-binding.sh
```

The command emits JSON with elapsed nanoseconds and the recognized-query count.
Timings are machine-local observations; equality of the count and successful
extension import are the correctness gates.

## Deferred prototypes

`ferrolex-python` is an evaluation prototype outside the current product
scope. There is no PyPI package, wheel matrix, or compatibility commitment.
The C ABI, LSP, and Visual Studio Code work are likewise retained prototype
history, not supported release surfaces. Their focused checks remain separate
from the Node.js release gate.

See [ADR-0010](adr/0010-external-integration-support-tiers.md).
