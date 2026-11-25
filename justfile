drover := env("DROVER", "drover")
test_allow := "-A clippy::unwrap_used -A clippy::expect_used -A clippy::panic -A clippy::indexing_slicing -A clippy::as_conversions -A clippy::print_stdout -A clippy::print_stderr -A clippy::let_underscore_must_use"

# Every gate, timed; CI runs this and nothing else. The last step fails when a step ran over ci/budget.toml.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    export FARRIER_TIMES="${DROVER_SCRATCH_TARGET:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}}/farrier-check-times"
    : >"$FARRIER_TIMES"
    start=$(date +%s)
    for gate in fmt clippy test fixtures deny actionlint cap arch; do
        scripts/timed.sh "$gate" just "$gate"
    done
    echo "check $(($(date +%s) - start))" >>"$FARRIER_TIMES"
    if [ -n "${FARRIER_JOB_START:-}" ]; then echo "ci-job $(($(date +%s) - FARRIER_JOB_START))" >>"$FARRIER_TIMES"; fi
    just budget "$FARRIER_TIMES"

# Every generated file (.drover/arch.toml `generated`); arch-check fails on a difference.
gen:
    scripts/gen-skill.sh
    rm -rf fixtures/units
    cargo run -q --locked -p farrier-service --example units -- fixtures/units

fmt:
    cargo fmt --check

# Pedantic and the shared deny lints; tests may unwrap, index and print.
clippy:
    cargo clippy --workspace --lib --bins --examples --locked -- -D warnings
    cargo clippy --workspace --tests --locked -- -D warnings {{test_allow}}

# Each crate's tests timed on their own against ci/budget.toml, after one build.
test:
    cargo fetch --locked
    cargo test --workspace --locked --no-run
    scripts/timed.sh test-farrier-service cargo test --locked -q -p farrier-service
    scripts/timed.sh test-farrier-update cargo test --locked -q -p farrier-update
    scripts/timed.sh test-farrier cargo test --locked -q -p farrier

# Each gate's must-fail fixture in ci/lint-fixtures fails its gate (`just fixtures cap` runs one).
fixtures *gates:
    scripts/selftest.sh {{gates}}

# Licences, advisories, duplicate versions and sources (deny.toml).
deny:
    cargo deny --locked check

actionlint:
    actionlint

# The line cap (ci/cap.toml plus ci/cap.d raises) on non-test, non-generated Rust, and the 30-line rules file.
cap:
    scripts/cap.sh . ci/cap.toml .drover/rules.md

# Layers, bans, sizes, comments, frozen and generated files; BASE and PR_BODY narrow the comment and doc checks to a PR.
arch:
    {{drover}} arch-check ${BASE:+--base "$BASE"} ${PR_BODY:+--pr-body "$PR_BODY"}

# The times table (also into the CI job summary); fails when a step ran over ci/budget.toml.
budget times:
    scripts/budget.sh {{times}}
