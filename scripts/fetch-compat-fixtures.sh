#!/usr/bin/env bash

# Downloads only digest-pinned compatibility fixtures into the supplied cache.
# Third-party dictionary bytes are intentionally never written into the repo.
set -euo pipefail

fixture_root=${1:?usage: fetch-compat-fixtures.sh <fixture-root>}
workspace_root=$(cd "$(dirname "$0")/.." && pwd)

for locale in en_US de_DE fr_FR nl_NL ar tr_TR; do
  cargo run --quiet -p ferrolex-cli -- dictionary fetch "$locale" --cache "$fixture_root"
done

hu_root="$fixture_root/hu_HU"
mkdir -p "$hu_root"
revision=f2ff99058268502bdcf4cad25c1ca2935ad8aa7d
base="https://raw.githubusercontent.com/LibreOffice/dictionaries/$revision/hu_HU"
curl --fail --location --retry 3 --silent --show-error --output "$hu_root/hu_HU.aff" "$base/hu_HU.aff"
curl --fail --location --retry 3 --silent --show-error --output "$hu_root/hu_HU.dic" "$base/hu_HU.dic"

cd "$workspace_root"
