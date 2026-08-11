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
      print "| Locale | Import gate | Checked recognition probes |"
      print "| --- | --- | --- |"
    }
    /^#/ { next }
    {
      gate = $11 == "strict" ? "strict fixture" : $11 == "lenient" ? "lenient fixture" : "blocked"
      probes = $12
      gsub(/;[^=]+=/, ", ", probes)
      sub(/^[^=]+=*/, "", probes)
      print "| `" $2 "` | " gate " | " probes " |"
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
