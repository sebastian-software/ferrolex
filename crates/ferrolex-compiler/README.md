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

<!-- ferramenta-family:registry:start -->
Part of the [Ferramenta](https://ferramenta.dev) family of Rust-native developer tools by [Sebastian Software](https://oss.sebastian-software.com).
Siblings: [ferroni](https://github.com/sebastian-software/ferroni), [ferriki](https://github.com/sebastian-software/ferriki), [ferromark](https://github.com/sebastian-software/ferromark), [ferralk](https://github.com/sebastian-software/ferralk), [ferrovia](https://github.com/sebastian-software/ferrovia), [ferrocat](https://github.com/sebastian-software/ferrocat), and [ferrugo](https://github.com/sebastian-software/ferrugo).
<!-- ferramenta-family:registry:end -->

<!-- sebastian-software-branding:start -->
<p align="center">
  <a href="https://oss.sebastian-software.com">
    <img src="https://raw.githubusercontent.com/sebastian-software/ferramenta/main/app/assets/logos/sebastian-software.svg" alt="Sebastian Software" width="240" />
  </a>
</p>

<p align="center">
  <a href="https://oss.sebastian-software.com">Open Source at Sebastian Software</a><br />
  Copyright &copy; 2026 Sebastian Software GmbH
</p>
<!-- sebastian-software-branding:end -->
