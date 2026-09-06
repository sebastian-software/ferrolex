#!/usr/bin/env bash
# Renders the Ferramenta family block into every published ferrolex README.
# Pass --check to report drift instead of rewriting.
#
# The block is generated, never maintained here: src/family.ts in
# sebastian-software/ferramenta is the single source of truth for family
# membership, job wording, grouping, and links, and its ferramenta-readme
# generator renders the block from it. The company footer below the block is a
# separate, standards-owned concern; scripts/mirror-readme-footer.py handles it.
#
# Needs network access, pnpm, and Node.js 22.13 or newer.
set -euo pipefail

# The generator is pinned so a check is reproducible: a branch ref would
# re-verify against whatever landed in ferramenta since, and the block CI
# blessed yesterday would not be the block it blesses today. A registry change
# reaches this repository by bumping this SHA and re-running the script; the
# README diff then shows exactly what the registry moved.
generator_ref='d63a0b163ef3e5e68cd1c77e5c8871ac72c36b60'
generator="github:sebastian-software/ferramenta#${generator_ref}&path:/packages/ardo-config"

# The READMEs that crates.io and npm render for a published ferrolex package.
# The unpublished FFI, LSP, and Python crates have no registry page and carry
# no block.
registry_readmes=(
  crates/ferrolex-core/README.md
  crates/ferrolex-dictionaries/README.md
  crates/ferrolex-code/README.md
  crates/ferrolex-suggest/README.md
  crates/ferrolex-text/README.md
  crates/ferrolex-compiler/README.md
  crates/ferrolex-hunspell/README.md
  crates/ferrolex-cli/README.md
  crates/ferrolex-node/README.md
)

mode='--write'
if [[ ${1:-} == --check ]]; then
  mode='--check'
elif [[ -n ${1:-} ]]; then
  echo "usage: $0 [--check]" >&2
  exit 2
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v pnpm > /dev/null 2>&1; then
  echo "$0: pnpm is required to run the family block generator" >&2
  exit 2
fi

status=0

# The root README is the GitHub project page and takes the full block with the
# grouped tables; every other surface takes the compact registry flavor.
pnpm dlx "$generator" --current ferrolex --variant github "$mode" README.md || status=1
for readme in "${registry_readmes[@]}"; do
  pnpm dlx "$generator" --current ferrolex --variant registry "$mode" "$readme" || status=1
done

exit "$status"
