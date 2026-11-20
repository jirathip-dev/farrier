#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
fx=ci/lint-fixtures
work="${DROVER_SCRATCH_TARGET:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}}/farrier-selftest"
bad=0
run() {
  local want=$1 name=$2 code=0 out
  shift 2
  out=$("$@" 2>&1) || code=$?
  if [ "$want" = pass ]; then
    if [ "$code" -eq 0 ]; then echo "ok: $name passes"; else echo "::error::$name failed (exit $code): $out"; bad=1; fi
  elif [ "$code" -ne 0 ] && grep -q -e "$want" <<<"$out"; then
    echo "ok: $name is refused"
  else
    echo "::error::$name did not fail on '$want' (exit $code): $out"
    bad=1
  fi
}
fresh() {
  rm -rf "${work:?}/$1"
  mkdir -p "$work/$1"
  echo "$work/$1"
}
gate_fmt() {
  run 'Diff in' "fmt: $fx/misformatted.rs" rustfmt --edition 2024 --check "$fx/misformatted.rs"
}
gate_clippy() {
  local t name
  t=$(fresh clippy)
  mkdir -p "$t/src"
  cp rust-toolchain.toml clippy.toml "$t/"
  {
    printf '[package]\nname = "lint-fixture"\nversion = "0.0.0"\nedition = "2024"\n\n[workspace]\n\n'
    awk '/^\[/ { on = ($0 ~ /^\[workspace\.lints\./) } on' Cargo.toml | sed 's/^\[workspace\.lints\./[lints./'
  } >"$t/Cargo.toml"
  for f in "$fx"/rust/*.rs; do
    name=$(basename "$f" .rs)
    cp "$f" "$t/src/lib.rs"
    if [ "$name" = clean ]; then
      run pass "clippy: $f" bash -c 'cd "$1" && cargo clippy -q --all-targets' _ "$t"
    else
      run "${name//_/[_-]}" "clippy: $f" bash -c 'cd "$1" && cargo clippy -q --all-targets' _ "$t"
    fi
  done
}
gate_deny() {
  local t
  t=$(fresh deny)
  mkdir -p "$t/unlisted/src" "$t/fixture"
  printf '[package]\nname = "unlisted"\nversion = "0.0.0"\nedition = "2024"\nlicense = "MIT"\n' >"$t/unlisted/Cargo.toml"
  : >"$t/unlisted/src/lib.rs"
  git -C "$t/unlisted" init -q
  git -C "$t/unlisted" add -A
  git -C "$t/unlisted" -c user.name=fixture -c user.email=fixture@example.invalid commit -qm fixture
  cp -R "$fx/deny/." "$t/fixture/"
  sed "s|@UNLISTED@|file://$t/unlisted|" "$fx/deny/Cargo.toml" >"$t/fixture/Cargo.toml"
  run "source-not-allowed.*'git' source" "deny: $fx/deny" cargo deny --manifest-path "$t/fixture/Cargo.toml" --config "$root/deny.toml" check sources
}
gate_actionlint() {
  run 'unexpected key "timeout-minute"' "actionlint: $fx/actionlint/unknown-key.yml" actionlint "$fx/actionlint/unknown-key.yml"
}
gate_cap() {
  run pass "cap: $fx/cap at the cap" sh scripts/cap.sh "$fx/cap/tree" "$fx/cap/at.toml" "$fx/cap/rules-30.md"
  run 'over the cap' "cap: $fx/cap one line over" sh scripts/cap.sh "$fx/cap/tree" "$fx/cap/over.toml" "$fx/cap/rules-30.md"
  run 'over 30' "cap: $fx/cap/rules-31.md" sh scripts/cap.sh "$fx/cap/tree" "$fx/cap/at.toml" "$fx/cap/rules-31.md"
}
gate_arch() {
  local t drover=${DROVER:-drover}
  t=$(fresh arch)
  git ls-files -z | xargs -0 tar -cf - | tar -xf - -C "$t"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=fixture -c user.email=fixture@example.invalid commit -qm fixture
  run pass "arch: the clean copy" bash -c 'cd "$1" && "$2" arch-check' _ "$t" "$drover"
  cp "$fx/arch/process.rs" "$t/crates/farrier-service/src/job.rs"
  run 'Command::new' "arch: $fx/arch/process.rs as farrier-service's job.rs" bash -c 'cd "$1" && git add -A && "$2" arch-check' _ "$t" "$drover"
  git -C "$t" checkout -q -- crates/farrier-service/src/job.rs
  cp "$fx/arch/stale-skill.rs" "$t/crates/farrier/src/gen/skill.rs"
  run 'differs from what' "arch: $fx/arch/stale-skill.rs as the generated skill list" bash -c 'cd "$1" && git add -A && "$2" arch-check' _ "$t" "$drover"
}
gate_budget() {
  run 'over budget' "budget: $fx/budget/over.times" sh scripts/budget.sh "$fx/budget/over.times"
}
for gate in "${@:-fmt clippy deny actionlint cap arch budget}"; do
  for g in $gate; do "gate_$g"; done
done
exit "$bad"
