//! Compile a Hunspell dictionary into a provenance-checked runtime cache and reload it.

use std::error::Error;

use ferrolex::hunspell::{compile_runtime_cache, load_runtime_cache, SourceDigests};
use ferrolex::{import_bytes, Dictionary, ImportMode};

fn main() -> Result<(), Box<dyn Error>> {
    let aff = b"SET UTF-8\n";
    let dic = b"1\nferrolex\n";
    let imported = import_bytes("example.aff", aff, "example.dic", dic, ImportMode::Strict)?;
    let sources = SourceDigests::from_source_bytes(aff, dic);
    let cache = compile_runtime_cache(imported.dictionary(), sources)?;
    let restored = load_runtime_cache(&cache, sources)?;

    assert!(restored.contains("ferrolex"));
    println!("reloaded {} cache bytes", cache.len());
    Ok(())
}
