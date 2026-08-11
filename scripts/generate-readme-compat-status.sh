#!/usr/bin/env bash
# Generates the concise README view from the pinned real-world fixture catalog.
# The detailed evidence and the oracle measurements remain in docs/.
set -euo pipefail

manifest='crates/ferrolex-hunspell/tests/real_world/manifest.tsv'
readme='README.md'
start='<!-- compat-status:start -->'
end='<!-- compat-status:end -->'

status_table=$(
  awk -F '\t' '
    BEGIN {
      print "| Dictionary locale | Status | What this means |"
      print "| --- | --- | --- |"
    }
    /^#/ { next }
    {
      if ($11 == "strict") {
        status = "✅ Ready for the tested core"
        meaning = "The pinned dictionary imports strictly and its reviewed word forms work."
      } else if ($11 == "lenient") {
        status = "🟡 In progress"
        meaning = "Common reviewed words work, but known dictionary features still need support."
      } else {
        status = "🔴 Blocked"
        meaning = "This exact dictionary cannot yet be imported reliably."
      }
      print "| `" $2 "` | " status " | " meaning " |"
    }
  ' "$manifest"
)

generated=$(mktemp)
table=$(mktemp)
trap 'rm -f "$generated" "$table"' EXIT
printf '%s\n' "$status_table" > "$table"
awk -v start="$start" -v end="$end" -v table="$table" '
  $0 == start {
    print
    while ((getline row < table) > 0) print row
    close(table)
    inside = 1
    next
  }
  $0 == end { inside = 0 }
  !inside { print }
' "$readme" > "$generated"

if [[ ${1:-} == --check ]]; then
  diff -u "$readme" "$generated"
  exit 0
fi

mv "$generated" "$readme"
trap - EXIT
rm -f "$table"
