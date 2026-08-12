#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$root/target"}

cargo +1.88 build --manifest-path "$root/crates/ferrolex-python/Cargo.toml" --release

case "$(uname -s)" in
  Darwin) library_extension=dylib ;;
  Linux) library_extension=so ;;
  *) echo "unsupported Python benchmark platform: $(uname -s)" >&2; exit 1 ;;
esac

module_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferrolex-python.XXXXXX")
trap 'rm -rf -- "$module_dir"' EXIT
cp "$target_dir/release/libferrolex_python.$library_extension" "$module_dir/ferrolex_python.so"

python3 "$root/crates/ferrolex-python/bench/lookup.py" "$module_dir"
