#!/bin/sh
set -eu
times=${1:?usage: budget.sh <times file> [budget.toml]}
budgets=${2:-ci/budget.toml}
over=0
table="| step | took (s) | budget (s) | |
|---|---|---|---|"
while read -r name took; do
  limit=$(sed -n "s/^$name *= *\([0-9]*\).*/\1/p" "$budgets")
  mark=ok
  if [ -z "$limit" ]; then
    limit=-
    mark=
  elif [ "$took" -gt "$limit" ]; then
    mark="over budget"
    over=1
  fi
  table="$table
| $name | $took | $limit | $mark |"
done <"$times"
echo "$table"
test -z "${GITHUB_STEP_SUMMARY:-}" || printf '### just check times\n\n%s\n' "$table" >>"$GITHUB_STEP_SUMMARY"
test "$over" -eq 0 || { echo "a step ran over $budgets" >&2; exit 1; }
