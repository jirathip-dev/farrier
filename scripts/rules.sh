#!/bin/sh
set -eu
rules=${1:?usage: rules.sh <rules.md>}
r=$(wc -l <"$rules" | tr -d ' ')
test "$r" -le 30 || { echo "$rules has $r lines, over 30" >&2; exit 1; }
echo "rules: $r / 30 lines"
