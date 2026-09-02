#!/usr/bin/env bash

# Verifies that the root package and current platform package install together
# in a clean, compiler-free consumer directory.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
package_root="$root/crates/ferrolex-node"
work=$(mktemp -d "${TMPDIR:-/tmp}/ferrolex-node-package.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

case "$(node -p 'process.platform + ":" + process.arch')" in
  darwin:arm64)
    suffix=darwin-arm64
    platform_directory=darwin-arm64
    ;;
  linux:x64)
    suffix=linux-x64-gnu
    platform_directory=linux-x64-gnu
    ;;
  win32:x64)
    suffix=win32-x64-msvc
    platform_directory=win32-x64-msvc
    ;;
  *)
    echo "unsupported Node package test platform" >&2
    exit 1
    ;;
esac

cd "$package_root"
npm run create-npm-dirs
mkdir -p "$work/packages" "$work/consumer"
cp "ferrolex_node.$suffix.node" "npm/$platform_directory/"

root_archive=$(npm pack --pack-destination "$work/packages" | tail -n 1)
platform_archive=$(
  npm pack "./npm/$platform_directory" --ignore-scripts --pack-destination "$work/packages" |
    tail -n 1
)

cd "$work/consumer"
npm init --yes >/dev/null
npm install --ignore-scripts --offline \
  "$work/packages/$root_archive" \
  "$work/packages/$platform_archive"

node -e '
  const { Checker, dictionaryCatalog } = require("@ferrolex/node")
  const checker = new Checker("ferrolex\nFerris")
  if (!checker.check("ferrolex") || checker.check("ferolex")) process.exit(1)
  if (checker.suggest("ferolex")[0] !== "ferrolex") process.exit(1)
  if (!dictionaryCatalog().some(({ locale }) => locale === "en_US")) process.exit(1)
'
