set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

toolchain := `python3 scripts/workspace-rust-version.py`
fuzz_toolchain := `python3 scripts/fuzz-toolchain.py`

# Fast feedback for the product crates most commonly changed together.
quick:
    cargo +{{toolchain}} fmt --all -- --check
    cargo +{{toolchain}} clippy --workspace --all-targets -- -D warnings
    cargo +{{toolchain}} test -p ferrolex-cli -p ferrolex-core -p ferrolex-hunspell -p ferrolex-suggest

# Deterministic CI parity. Network-, credential-, and licensed-fixture-dependent
# jobs remain explicit workflow/local commands rather than hidden prerequisites.
gate: quick
    RUSTDOCFLAGS="-D warnings" cargo +{{toolchain}} doc --workspace --no-deps
    cargo +{{toolchain}} test --workspace
    cargo +{{toolchain}} bench -p ferrolex-core --no-run
    cargo +{{toolchain}} bench -p ferrolex-compiler --no-run
    cargo +{{toolchain}} bench -p ferrolex-suggest --no-run
    cargo +{{toolchain}} bench -p ferrolex-hunspell --no-run
    FERROLEX_SUGGESTION_QUALITY_REQUIRE_APPROVED=1 cargo +{{toolchain}} test -p ferrolex-suggest --test suggestion_quality -- --nocapture
    test -s LICENSE-APACHE
    test -s LICENSE-MIT
    cargo deny check
    python3 scripts/check-release-version-contract.py
    python3 scripts/publish-crates.py --check

# Optional local smoke coverage; install cargo-fuzz for the pinned nightly first.
fuzz-smoke:
    for target in hunspell_import runtime_cache_loader compiled_loader suggestion_input compound_evaluation analyze_source project_config word_list; do RUSTUP_TOOLCHAIN={{fuzz_toolchain}} cargo fuzz run "$$target" -- -runs=256; done
