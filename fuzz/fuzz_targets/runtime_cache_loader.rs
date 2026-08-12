#![no_main]

use ferrolex_hunspell::{
    inspect_runtime_cache, load_runtime_artifact, load_runtime_cache, SourceDigests,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let sources = SourceDigests::from_source_bytes(data, data);
    let _ = inspect_runtime_cache(data);
    let _ = load_runtime_artifact(data);
    let _ = load_runtime_cache(data, sources);
});
