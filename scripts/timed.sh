#!/bin/sh
set -eu
name=$1
shift
start=$(date +%s)
"$@"
test -z "${FARRIER_TIMES:-}" || printf '%s %s\n' "$name" "$(($(date +%s) - start))" >>"$FARRIER_TIMES"
