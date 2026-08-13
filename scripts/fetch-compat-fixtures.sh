#!/usr/bin/env bash

# Downloads only digest-pinned compatibility fixtures into the supplied cache.
# Third-party dictionary bytes are intentionally never written into the repo.
set -euo pipefail

fixture_set=all
if [[ ${1:-} == --set ]]; then
  fixture_set=${2:?usage: fetch-compat-fixtures.sh [--set required|scorecard|all] <fixture-root>}
  shift 2
fi

fixture_root=${1:?usage: fetch-compat-fixtures.sh [--set required|scorecard|all] <fixture-root>}
workspace_root=$(cd "$(dirname "$0")/.." && pwd)

case "$fixture_set" in
  required)
    locales=(en_US de_DE ar)
    ;;
  scorecard)
    locales=(en_US de_DE fr_FR nl_NL ar tr_TR)
    ;;
  all)
    locales=(en_US de_DE es_ES fr_FR it_IT pt_BR pt_PT nl_NL pl_PL ar tr_TR)
    ;;
  *)
    echo "unknown fixture set \`$fixture_set\`; expected required, scorecard, or all" >&2
    exit 2
    ;;
esac

for locale in "${locales[@]}"; do
  cargo run --quiet -p ferrolex-cli -- dictionary fetch "$locale" --cache "$fixture_root"
done

hu_root="$fixture_root/hu_HU"
mkdir -p "$hu_root"
revision=f2ff99058268502bdcf4cad25c1ca2935ad8aa7d
base="https://raw.githubusercontent.com/LibreOffice/dictionaries/$revision/hu_HU"
curl --fail --location --retry 3 --silent --show-error --output "$hu_root/hu_HU.aff" "$base/hu_HU.aff"
curl --fail --location --retry 3 --silent --show-error --output "$hu_root/hu_HU.dic" "$base/hu_HU.dic"

cd "$workspace_root"
