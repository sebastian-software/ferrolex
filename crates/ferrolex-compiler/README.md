# ferrolex-compiler

Supporting compiled dictionary format and deployment tools for [ferrolex](https://github.com/sebastian-software/ferrolex).

The format is deterministic, bounds-checked, little-endian, and suitable for
loading from a memory-mapped backing store. It is an implementation boundary,
not a separate product promise.

```rust
use ferrolex_compiler::{compile_words, CompiledDictionary};
use ferrolex_core::Dictionary;

let bytes = compile_words(["ferrolex"])?;
let dictionary = CompiledDictionary::load(bytes)?;
assert!(dictionary.contains("ferrolex"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

See the [API documentation](https://docs.rs/ferrolex-compiler) and the
[workspace architecture](https://github.com/sebastian-software/ferrolex/blob/main/ARCHITECTURE.md).

<!-- ferramenta-family:start -->
**ferrolex** is part of the [Ferramenta](https://ferramenta.dev) family — Rust-native developer tools that keep the APIs the ecosystem already knows.

Siblings: [ferroni](https://sebastian-software.github.io/ferroni/) · [ferriki](https://github.com/sebastian-software/ferriki) · [ferromark](https://sebastian-software.github.io/ferromark/) · [ferrocat](https://ferrocat.dev) · [palamedes](https://palamedes.dev) · [ferrovia](https://github.com/sebastian-software/ferrovia) · [ferralk](https://github.com/sebastian-software/ferralk) · [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:end -->

<!-- sebastian-software-branding:start -->

<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://sebastian-brand.vercel.app/sebastian-software/logo-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <strong>Built by Sebastian Software</strong> — consulting for TypeScript, React &amp; Rust.<br />
  <a href="https://sebastian-software.de">Work with us</a> · <a href="https://oss.sebastian-software.com">More open source</a>
</p>

<p align="center">Copyright &copy; 2026 Sebastian Software GmbH</p>

<!-- sebastian-software-branding:end -->
