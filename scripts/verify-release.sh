#!/usr/bin/env bash
set -euo pipefail

work_directory="$(mktemp -d "${TMPDIR:-/tmp}/ferrolex-release.XXXXXX")"
trap 'rm -rf "$work_directory"' EXIT

word_list="$work_directory/words.txt"
first_artifact="$work_directory/first.flex"
second_artifact="$work_directory/second.flex"

printf '%s\n' 'ferrolex' 'Straße' '東京' > "$word_list"

cargo +1.88 build --workspace --release --locked
cargo +1.88 run --locked --release -p ferrolex-cli -- compile --dictionary "$word_list" -o "$first_artifact"
cargo +1.88 run --locked --release -p ferrolex-cli -- compile --dictionary "$word_list" -o "$second_artifact"
cmp "$first_artifact" "$second_artifact"
cargo +1.88 run --locked --release -p ferrolex-cli -- validate --compiled "$first_artifact"
