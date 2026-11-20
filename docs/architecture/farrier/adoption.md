# Adoption: inventory, order, and the agent-speed criteria

Line counts are from each repo's integration branch on 2026-11-20. "Kept" lines are estimates.

## Inventory

| repo | file (lines) | goes to | kept in the repo |
|---|---|---|---|
| cooee | `crates/cooee/src/update.rs` (262) | `Updater::{run, plan, status}`, `contract`, `Reject` | about 60: the `Product`, the `Supervised` wiring, the failure push, the log line, `--version` |
| cooee | `crates/cooee/src/service.rs` (120) | `Job`, `Service::{files, write}` replace `plist`, `write_plist`, `xml`, `job` | about 45: `label`, the job values, `tailnet_name` and `app_items` (neither is job install) |
| cooee | `crates/cooee/src/setup.rs`, `service()` (36 of the file) | `Service::install`, `restart` | the step line and the wait for the port |
| cooee | `crates/cooee/src/cmd/install.rs`, the plist lines | `Service::write`, `commands` | the rest |
| shed | `crates/shed/src/update.rs` (202) | `Updater`, `stage`, `swap`, `state` | about 70: the flags, `Serve: Daemon` (the two handover calls), the status line |
| shed | `crates/shed/src/service.rs` (260) | `Service::{install, uninstall, start, status, commands}` replace `jobs`, `install`, `sh`, `start_job`, `uninstall`, `written` | about 135: `doctor` (its rows read `status()`), the two job values |
| shed | `dist/launchd/*.plist` (2 files), `dist/systemd/*` (3 files) | rendered by `farrier-service`; golden copies in farrier's `fixtures/units/` | nothing |
| drover | `crates/core/src/release.rs` (251) | `contract`, `stage`, `swap`, `Reject`, `Builds`, `fetch` | `failure` (32 lines: the release workflow's first error, for the alert), moved into the rollout file |
| drover | `crates/core/src/update.rs` (627) | the per-host parts: staging a member, the swap rename, the remote shell lines, the restart retry | about 480 as `rollout.rs`: `members`, `step`, the marks and events, `plan`, pause, the staging-serve target as a second `Updater`, the handover client |
| drover | `crates/core/src/timer.rs` (790) | `files`, `ctl`, `refused`, `rebootstrap`, `restart`, `restart_cmd`, `job_pid`, `refused_jobs`, `heal`, `drift`, `states`, `install` (about 215) go to `Service` | about 575: the job table as `Job` values, `tick`, `setup`, `services`, `repair`, the PATH discovery |
| drover | `assets/job.plist`, `job.service`, `job.timer` (27) | rendered by `farrier-service` | nothing |
| drover | `crates/core/src/service.rs` (134) | **nothing: kept as it is** | all of it. It is the service's socket and the lane tokens, not job install; the job install the survey meant is in `timer.rs` |
| beacon | `cli/src/install.rs` (90), later | `Job`, `Service::install` | the flags |
| all three | `.github/workflows/release.yml`, `scripts/release-sign.sh` | gatekeeper's release workflow, under [contract.md](contract.md) | the call |

About 1,200 lines leave cooee, shed and drover (about 295, 310 and 600), and about 90 more with beacon. farrier is
about 1,650 lines under a cap of 2,000. Tests of behaviour farrier now owns move with it; they are not copied.

## Before and after, per consumer

| | before | after |
|---|---|---|
| cooee | commits API for the head; checksum; restart; rolls back; marks a build failed on any error; macOS only; `install` prints the load command | `ls-remote`; signature, checksum, code signature; the same roll back; only a real failure marks a build; Linux jobs for free; `Service::install` loads it |
| shed | check with serve, swap, hand over; no undo; a status file; job texts from `dist/`; `uninstall` prints commands | the same check as `admit`, the same handover as `restart` (`Later` when serve is busy); a roll back if the new build never serves; `state.json`; jobs from values; a real `uninstall`; a refused job is re-bootstrapped |
| drover | stage by a shell line per host; `step`; swap by `mv` over ssh; restart, alert, no undo; jobs from three templates; drift repair | stage and swap by `--phase` on each host; `step` unchanged; the main host's swap confirms or rolls back; jobs from values; drift is `status()`, repair is `install()` |

The callers' code for each is in [README.md](README.md).

## Order

| # | repo | what | needs |
|---|---|---|---|
| F1 | farrier | workspace, every gate green on an empty crate, `farrier skill --to` | nothing |
| F2 | farrier | `farrier-service`, a golden file for every consumer job | F1 |
| F3 | farrier | the contract, `farrier release verify`, farrier's own first releases, signed with the release key, the offline backup key compiled in | F1 |
| F4 | farrier | `farrier-update`: the machine, `Supervised`, `phase`, `fetch` | F2, F3 |
| G1 | gatekeeper | its release workflow signs `SHA256SUMS` and runs the checker (gatekeeper's issue) | F3 |
| C1, C2 | cooee | service, then update | F2; F4 and G1 |
| S1, S2 | shed | service, then update | C1; C2 |
| D1, D2 | drover | service, then update | S1; S2 |
| B1 | beacon | `beacon install` | D1 |

- cooee first: the smallest copy, already closest to the target (it rolls back), and a bad update there drops no
  lane. shed second: it adds Linux, timers and the handover hooks, and it is what lanes run in, so it follows a
  proven API. drover last: the most code, several hosts, and it installs itself with what it adopts.
- Service before update in each repo: two small PRs, and the updater restarts through the service.
- Every adoption PR deletes what it replaces in the same PR, adds the three bans from [adr.md](adr.md), and shows
  the rendered jobs equal to the old texts, or names each difference.

## The agent-speed criteria

| criterion | mechanism |
|---|---|
| 1. One gate, same everywhere | `just check` is the only thing CI runs (after installing the pinned tools) and is the fleet's `done_check`. The platform is a value, not a compile flag (`Env`, `Target`; a ban keeps `target_os` in two files), so the macOS and Linux CI jobs and a lane's machine all compile and run the same tests, both renderers and both control paths. Tests reach no network and no real launchd or systemd: stub `launchctl`, `systemctl`, `curl` and `git` in a temp folder, a release as a local folder. Once gatekeeper's core and rust packs exist, `just check` calls them and farrier's copies of the shared fixtures are deleted |
| 2. A skill | `skill/farrier/`: `SKILL.md` (which crate for which job), `service.md` (each `Job` field and the plist and unit keys it writes), `update.md` (`Daemon`, every `Outcome` line, the state files, the phases), `contract.md`, and `examples/` for a daemon, a plain CLI and a rollout. The examples are generated from `crates/*/examples/*.rs`, which `just check` compiles. `farrier skill --to <dir>` writes the folder; a consumer lane's brief gets it the way blocks' skill arrives |
| 3. Fix once | the inventory above, deleted in the adoption PR itself. Three bans in each consumer (`launchctl`, `systemctl`, `SHA256SUMS` outside farrier) make a second copy fail arch-check |
| 4. Smaller blast radius | about 1,200 lines leave the consumers. drover's two hot files shrink or split: `timer.rs` loses its job-install half, `update.rs` becomes the rollout alone. A launchd, systemd or release-format fix no longer touches a consumer at all |
| 5. Fast | `ci/budget.toml`, checked by the last step of `just check` from the times each step recorded, printed as a table in the CI summary: tests of `farrier-service` 5 s, `farrier-update` 15 s, `farrier` 5 s; all of `just check` warm 90 s; the CI job cold 5 min. No test sleeps: only `ctl` and `tool` may wait, and tests replace both. A consumer's cost has a budget too: `farrier-update` adds at most 12 crates to a lock file, counted by a test. gatekeeper's timer replaces the local step when it ships |

## Gates from the first commit, and the fixture that fails each

| gate | must-fail fixture |
|---|---|
| `cargo fmt --check` | a misformatted file under `ci/lint-fixtures/` |
| clippy pedantic and the shared deny lints | one file per lint in `ci/lint-fixtures/rust/` |
| cargo-deny (licences, advisories, duplicates, sources) | a fixture manifest with a git source that is not listed |
| actionlint | a workflow with an unknown key |
| the line cap (`ci/cap.toml`, 2,000) and the 30-line rules file | a fixture tree one line over |
| `drover arch-check` (layers, bans, sizes, comments, frozen, generated) | a file that starts a process outside `ctl`; a stale generated file |
| the release contract | one folder per case in `fixtures/releases/`, each naming the `Reject` it must get: wrong checksum, no signature, another key, another release's signed list, an extra line, a missing asset, a `--version` naming another build |
| a job's rules | a bad label, a relative program path, hour 24: `Job::new` refuses each |
| the time budget | a times file over budget |
