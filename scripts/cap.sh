#!/bin/sh
set -eu
root=${1:?usage: cap.sh <root> <cap.toml> <rules.md>}
caps=${2:?usage: cap.sh <root> <cap.toml> <rules.md>}
rules=${3:?usage: cap.sh <root> <cap.toml> <rules.md>}
cap=$(cat "$caps" "$(dirname "$caps")"/cap.d/*.toml 2>/dev/null | sed -n 's/^line_cap *= *\([0-9]*\).*/\1/p' | sort -n | tail -n 1)
test -n "$cap" || { echo "line_cap missing from $caps" >&2; exit 2; }
warn=$(sed -n 's/^warn_within *= *\([0-9]*\).*/\1/p' "$caps")
n=$(find "$root"/crates/*/src -name '*.rs' ! -path '*/src/gen/*' -exec awk '/^[[:space:]]*#\[cfg\(test\)\]/ { exit } { n++ } END { print n + 0 }' {} \; 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')
echo "non-test lines: $n / cap $cap"
left=$((cap - n))
test "$left" -ge 0 || { echo "$((-left)) lines over the cap in $caps" >&2; exit 1; }
test "$left" -ge "${warn:-200}" || echo "WARN: $left lines of headroom (< ${warn:-200})"
r=$(wc -l <"$rules" | tr -d ' ')
test "$r" -le 30 || { echo "$rules has $r lines, over 30" >&2; exit 1; }
echo "rules: $r / 30 lines"
