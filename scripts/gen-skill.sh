#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
out=crates/farrier/src/gen/skill.rs
{
  echo 'pub const FILES: &[(&str, &str)] = &['
  (cd skill/farrier && find . -type f -name '*.md' | sed 's|^\./||' | LC_ALL=C sort) | while read -r f; do
    printf '("%s", include_str!("../../../../skill/farrier/%s")),\n' "$f" "$f"
  done
  echo '];'
} >"$out"
rustfmt --edition 2024 "$out"
