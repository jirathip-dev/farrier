#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
rm -rf skill/farrier/examples
mkdir -p skill/farrier/examples
for f in crates/*/examples/*.rs; do
  name=$(basename "$f" .rs)
  test "$name" != units || continue
  printf '# %s\n\nFrom `%s`, compiled by `just check`.\n\n```rust,ignore\n' "$name" "$f" >"skill/farrier/examples/$name.md"
  cat "$f" >>"skill/farrier/examples/$name.md"
  echo '```' >>"skill/farrier/examples/$name.md"
done
out=crates/farrier/src/gen/skill.rs
{
  echo 'pub const FILES: &[(&str, &str)] = &['
  (cd skill/farrier && find . -type f -name '*.md' | sed 's|^\./||' | LC_ALL=C sort) | while read -r f; do
    printf '("%s", include_str!("../../../../skill/farrier/%s")),\n' "$f" "$f"
  done
  echo '];'
} >"$out"
rustfmt --edition 2024 "$out"
