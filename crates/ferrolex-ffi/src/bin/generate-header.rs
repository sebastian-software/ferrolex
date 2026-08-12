//! Regenerates the checked-in experimental C header.

use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = crate_dir.join("include/ferrolex.h");

    cbindgen::generate(&crate_dir)
        .unwrap_or_else(|error| panic!("could not generate the ferrolex C header: {error}"))
        .write_to_file(output);
}
