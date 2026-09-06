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
