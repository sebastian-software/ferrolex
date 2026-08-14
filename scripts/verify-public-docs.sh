#!/usr/bin/env bash
set -euo pipefail

documentation_target="$(mktemp -d "${TMPDIR:-/tmp}/ferrolex-public-docs.XXXXXX")"
trap 'rm -rf "$documentation_target"' EXIT

CARGO_TARGET_DIR="$documentation_target" cargo +1.88 doc --locked --no-deps \
  -p ferrolex \
  -p ferrolex-core \
  -p ferrolex-text \
  -p ferrolex-code \
  -p ferrolex-hunspell \
  -p ferrolex-suggest \
  -p ferrolex-dictionaries \
  -p ferrolex-compiler \
  -p ferrolex-ffi \
  -p ferrolex-node \
  -p ferrolex-python \
  -p ferrolex-lsp

for package in \
  ferrolex \
  ferrolex_core \
  ferrolex_text \
  ferrolex_code \
  ferrolex_hunspell \
  ferrolex_suggest \
  ferrolex_dictionaries \
  ferrolex_compiler \
  ferrolex_ffi \
  ferrolex_node \
  ferrolex_python \
  ferrolex_lsp; do
  test -s "$documentation_target/doc/$package/index.html"
done

grep -Fq 'Supported public Rust API.' "$documentation_target/doc/ferrolex/index.html"
