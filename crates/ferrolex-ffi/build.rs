use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=include/ferrolex.h");

    if env::var_os("CARGO_FEATURE_C_ABI").is_none() {
        return;
    }

    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR");
    let generated_header =
        PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR for build scripts"))
            .join("ferrolex.h");

    cbindgen::generate(&crate_dir)
        .unwrap_or_else(|error| panic!("could not generate the ferrolex C header: {error}"))
        .write_to_file(&generated_header);

    let checked_header = PathBuf::from(crate_dir).join("include/ferrolex.h");
    if checked_header.exists() {
        let generated = fs::read(&generated_header).expect("generated C header is readable");
        let checked = fs::read(&checked_header).expect("checked-in C header is readable");

        assert_eq!(
            generated, checked,
            "the checked-in C header is stale; regenerate it with `cargo run -p ferrolex-ffi --features c-abi --bin generate-header`"
        );
    }
}
