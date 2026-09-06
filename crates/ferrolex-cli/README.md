# ferrolex-cli

Reference and diagnostic command-line interface for [ferrolex](https://github.com/sebastian-software/ferrolex).

The binary checks plain text and source files, imports or validates Hunspell
dictionaries, manages reviewed dictionary caches, and exposes machine-readable
JSON output for automation.

```sh
cargo run --bin ferrolex -- check --dictionary words.txt README.md
cargo run --bin ferrolex -- dictionary list
```

The CLI is a supporting interface. Library consumers should use the public
Rust API from the [`ferrolex`](https://docs.rs/ferrolex) crate directly.
