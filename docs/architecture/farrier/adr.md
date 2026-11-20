# ADR: farrier, one self-update and one service-install for cooee, drover and shed (#1)

## Summary

Decision: two library crates and one small tool. The service crate writes, loads, restarts and removes a launchd or systemd job from one typed description. The update crate finds the release of a branch's head, verifies it (signed checksum list, checksum, macOS code signature, the build it reports), swaps it in with one rename, restarts the daemon, and puts the old binary back if the new one does not serve. drover's several-host rollout stays in drover and drives the same two steps (stage, swap) on each host.
Slices: (1) the repo with every gate and the skill command; (2) the service crate; (3) the release contract and its checker; (4) the update crate; (5) adoption in cooee, then shed, then drover, two PRs each; beacon's CLI later.
Open questions: (1) The signature needs one new release secret, a signing key whose public half is compiled into each daemon: approve it (recommended), or use GitHub's keyless attestations (no secret, but every host then needs the GitHub CLI signed in), or keep Linux builds unsigned as today?

**Status:** proposed, 2026-11-20. Usage, shapes, boundaries and statecharts: [README.md](README.md). The release
layout: [contract.md](contract.md). Inventory, order and the agent-speed criteria: [adoption.md](adoption.md).

## The question

Three daemons each install themselves as a launchd or systemd job and each update themselves from their GitHub
releases. The three copies agree on the outline and differ in every detail that was learned the hard way:

| | cooee | shed | drover |
|---|---|---|---|
| the head | commits API (60 calls an hour) | `git ls-remote` | `git fetch` in a clone |
| checksum | in process | `sha256sum` or `shasum` | a shell line |
| code signature checked | no | no | yes (macOS) |
| before the swap | nothing | serve is asked "can this build take over?" | the staged binary's config check |
| after a failed start | rolls back | nothing | an alert, the new binary stays |
| a job launchd refuses after a swap | not handled | not handled | re-bootstrapped, 3 tries |
| Linux jobs | none | yes | yes |
| uninstall | none | prints the commands | removes |

None verifies who published the release. So: what is the one shape of "a job" and "an update" that covers all
three, where does it live, what must a release look like, and how does a rollout over several hosts fit?

## Options

**The split**

| | option | verdict |
|---|---|---|
| A | one crate, two features | every gate then runs per feature set, and a CLI that only installs a job still compiles the updater |
| B | **`farrier-service`, `farrier-update`, and the `farrier` tool** | chosen: update depends on service, never the reverse; the tool carries the skill and the release checker |
| C | B plus a shared core crate (process runner, platform) | the shared part is about 40 lines; a third crate to version costs more |
| D | share only the templates and the contract, keep the code per repo | the bugs are in the code, not the templates |

**A job: templates or types.** shed and drover fill placeholders in template files; cooee formats a string. A
template cannot say "a timer needs an interval", and three of the four live differences (keep-alive policy, process
type, what happens to child processes) are keys someone forgot in one copy. Chosen: a typed `Job`, rendered by
farrier, with a golden file per consumer job so an adoption PR shows that the rendered text did not change.

**Where the head comes from.** GitHub's "latest release" is ordered by publish time, so two CI runs that finish out
of order step a host back, and it cannot name a branch's prerelease. The commits API has a quota. Chosen:
`git ls-remote`, then the release by the tag built from that sha (shed's decision, now everyone's).

**The signature**

| | option | verdict |
|---|---|---|
| A | none: TLS, the checksum list, macOS code signing | as today; a Linux build is unsigned, and the checksum list is as trusted as the download beside it |
| B | **a minisign signature of `SHA256SUMS`, public keys compiled in** | chosen, if the owner approves the secret: verified in process on both platforms by `minisign-verify` (MIT, no dependencies); its signed comment ties the list to one repo, tag and commit |
| C | GitHub artifact attestations | no secret to hold, but verifying needs the GitHub CLI and a token on every host, or a large verifier in every daemon |
| D | a signature per asset | more files and the same trust as B |

**Network and hashing.** A TLS client crate adds dozens of crates to three daemons and to farrier's licence and
advisory gate. Chosen: `curl` and `git` as today, started in one module, and SHA-256 and the signature in process.

**After the swap**

| | option | verdict |
|---|---|---|
| A | check before the swap, no undo (shed today) | a build that passes the check and then fails to serve stays live, and the job restarts it forever |
| B | swap, restart, alert (drover today) | the same, with an alert |
| C | **check before, confirm after, roll back** | chosen: the consumer's `admit` runs before the swap, `serves` after it; without a yes in time the old file goes back and that build is skipped. The owner fixed "roll back on a failed start" |

**Several hosts**

| | option | verdict |
|---|---|---|
| A | the rollout (who waits for whom, lag limits, events) in farrier | one consumer has it, and it reads drover's host list, cloud states and event log |
| B | drover stages remote hosts with its own shell line (today) | a second implementation of download and verify, and a shell line cannot check the signature |
| C | **two explicit phases in farrier, the rollout in drover** | chosen: `stage` and `swap` rest on disk between calls; each host runs them with its own binary; drover decides when |

**How consumers depend on it.** crates.io publishing is outward-facing and cannot be undone. Chosen: a git
dependency pinned by `rev`, with the repo on each consumer's `allow-git` list, as the fleet's other shared crates
are.

## Decision

1. **Three packages.** `farrier-service` (std only), `farrier-update` (depends on it, plus a SHA-256 crate, a
   minisign verifier and serde), and the `farrier` tool (`skill --to <dir>`, `release verify`).
2. **A job is a typed value** with one name on both platforms. Every verb is idempotent. The platform is a value
   too (`Env`), so both renderers and both control paths are compiled and tested on every OS.
3. **A release follows [contract.md](contract.md):** today's tags and asset names, `SHA256SUMS`, and a new
   `SHA256SUMS.minisig`. gatekeeper's workflow produces it and runs `farrier release verify` before it publishes.
4. **One update machine** ([README.md](README.md), first statechart): check, download, verify, staged, swap,
   restart, confirm or roll back. `Staged` and `Swapped` are resting states on disk. The consumer supplies three
   answers through `Daemon`: may it take over, put it in service, does it serve.
5. **A rollout is drover's policy over farrier's phases.** drover keeps `step`, its limits
   (`self_update_wait_minutes`, `_lag_minutes`, `_stage_secs`, `_swap_secs`) and its events. Each host stages and
   swaps with its own binary through a `--phase` verb; a host with no binary gets one fetched and verified on the
   main host.
6. **farrier says, the consumer tells.** The crates return an `Outcome` and print, log and alert nothing.
7. **Adoption is cooee, shed, drover**, service first and update second in each, every PR deleting what it
   replaces ([adoption.md](adoption.md)).

## Consequences

- **Behaviour changes, on purpose.** shed and drover gain the roll back and the signature check; cooee gains the
  code-signature check, the check before the swap, Linux jobs, and stops calling a quota-limited API; cooee and
  shed gain the re-bootstrap of a job launchd refuses; shed gains a real `uninstall`. cooee no longer marks a
  build failed for a download error.
- **The signature is switched on in order.** First gatekeeper's workflow publishes `SHA256SUMS.minisig` (old
  updaters ignore it). Then a consumer adopts `farrier-update` with the public key compiled in; from that build on
  the signature is required. The reverse order would strand hosts. The open question settles slice 3: with
  "unsigned as today", `Product.keys`, `Reject::Signature` and check 3 of the contract are left out, not switched
  off.
- **No manual hop.** The tags, asset names and `--version` form stay as they are, so each host's old updater
  installs the first farrier build.
- **One name per job, so shed's Linux units are renamed** (`shed.service` to `dev.shed.serve.service`) through
  `supersedes`. That is one cold restart of serve on a Linux host, at the moment the owner runs `shed install`
  there; the self-update never reinstalls jobs. `shed doctor` shows the fix until then.
- **Hosts need `git` and `curl`**, as today. `Updater::run` reports a missing one as an `Error`, and each
  consumer's doctor shows it.
- **Per-user jobs only.** No LaunchDaemons, no system units, no Windows.
- **The templates go** (`dist/`, `assets/job.*`). A user who wants the text runs the tool's install with its
  dry-run flag; the golden files in farrier are the reviewed copies.
- **drover's remote hosts update through their own binary.** A host whose binary predates the `--phase` verb is
  caught up once by the fetch-and-copy path that also starts new hosts.
- **farrier depends on gatekeeper for nothing at build time**, and gatekeeper only runs the released `farrier`
  tool. So farrier publishes its own releases under the same contract, and its CI checks them with itself.
- **Size.** About 1,650 lines in farrier (cap 2,000) against about 1,200 leaving the three consumers: more code in
  total, because every consumer gains what only one had, and the next fix lands once. The numbers per file are in
  [adoption.md](adoption.md).
- **Hot files.** farrier starts with none: its lists are folders (below). In the consumers, drover's `timer.rs`
  (43 commits in 14 days) and `update.rs` (24) are the hot ones this touches; adoption takes the job-install half
  out of the first and splits the second into the rollout and nothing else. Neither is a central list or a
  generated file, so no consumer file is frozen by this ADR.

## `.drover/arch.toml`

farrier's own, added in slice 1 with the first code:

```toml
frozen = ["ci/cap.toml"]
generated = ["fixtures/units/*", "skill/farrier/examples/*.md", "crates/farrier/src/gen/skill.rs"]
generate = "just gen"

[[collect]]
dir = "ci/cap.d"
glob = "*.toml"

[[collect]]
dir = "fixtures/releases"
glob = "*/case.toml"

[[collect]]
dir = "ci/lint-fixtures/rust"
glob = "*.rs"

[[collect]]
dir = "skill/farrier"
glob = "*.md"

[comments]
policy = "strict"
except = ["ci/lint-fixtures/**"]

[docs]
paths = ["docs/*.md", "README.md", "skill/farrier/*.md"]
skip = [".drover/**"]

[[size]]
paths = ["crates/*/src/**/*.rs"]
max_file_lines = 400
max_fn_lines = 60

[[ban]]
pattern = "Command::new"
paths = ["crates/*/src/**"]
except = ["crates/farrier-service/src/ctl.rs", "crates/farrier-update/src/tool.rs"]
reason = "#1: processes start only in ctl (launchctl, systemctl) and tool (git, curl, codesign, a staged binary)"

[[ban]]
pattern = "thread::sleep"
paths = ["crates/*/src/**"]
except = ["crates/farrier-service/src/ctl.rs", "crates/farrier-update/src/tool.rs"]
reason = "#1: only ctl and tool wait, so every other module is tested without a clock"

[[ban]]
pattern = "target_os"
paths = ["crates/*/src/**"]
except = ["crates/farrier-service/src/env.rs", "crates/farrier-update/src/id.rs"]
reason = "#1: the platform is a value (Env, Target), so every OS compiles and tests both"

[[layer]]
name = "job"
paths = ["crates/farrier-service/src/job.rs"]
mods = ["crate::job"]

[[layer]]
name = "env"
paths = ["crates/farrier-service/src/env.rs"]
mods = ["crate::env"]

[[layer]]
name = "render"
paths = ["crates/farrier-service/src/render/**"]
mods = ["crate::render"]
may_use = ["job"]

[[layer]]
name = "ctl"
paths = ["crates/farrier-service/src/ctl.rs"]
mods = ["crate::ctl"]
may_use = ["job"]

[[layer]]
name = "service"
paths = ["crates/farrier-service/src/service.rs"]
mods = ["crate::service", "farrier_service"]
may_use = ["job", "env", "render", "ctl"]

[[layer]]
name = "id"
paths = ["crates/farrier-update/src/id.rs"]
mods = ["crate::id"]

[[layer]]
name = "contract"
paths = ["crates/farrier-update/src/contract.rs"]
mods = ["crate::contract", "farrier_update::contract"]
may_use = ["id"]

[[layer]]
name = "tool"
paths = ["crates/farrier-update/src/tool.rs"]
mods = ["crate::tool"]
may_use = ["id"]

[[layer]]
name = "stage"
paths = ["crates/farrier-update/src/stage.rs"]
mods = ["crate::stage"]
may_use = ["id", "contract", "tool"]

[[layer]]
name = "swap"
paths = ["crates/farrier-update/src/swap.rs"]
mods = ["crate::swap"]
may_use = ["id"]

[[layer]]
name = "state"
paths = ["crates/farrier-update/src/state.rs"]
mods = ["crate::state"]
may_use = ["id"]

[[layer]]
name = "updater"
paths = ["crates/farrier-update/src/updater/**"]
mods = ["crate::updater", "farrier_update"]
may_use = ["id", "contract", "tool", "stage", "swap", "state", "service"]
```

Each consumer's adoption PR adds these to its own `.drover/arch.toml`, with its issue number, and drops any older
ban they replace (drover's `releases/download` exception for `release.rs`):

```toml
[[ban]]
pattern = "launchctl"
paths = ["crates/*/src/**"]
reason = "#N: jobs are written, loaded and restarted only by farrier-service"

[[ban]]
pattern = "systemctl"
paths = ["crates/*/src/**"]
reason = "#N: jobs are written, loaded and restarted only by farrier-service"

[[ban]]
pattern = "SHA256SUMS"
paths = ["crates/*/src/**"]
reason = "#N: a release is found, verified and swapped only by farrier-update"
```
