#![no_main]

use ferrolex_compiler::{inspect_compiled_artifact, CompiledDictionary};
use ferrolex_core::Dictionary;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = inspect_compiled_artifact(data);
    if let Ok(dictionary) = CompiledDictionary::load(data.to_vec()) {
        let _ = dictionary.validate();
        let _ = dictionary.contains("fuzz-query");
        let _ = dictionary.words().count();
    }
});
