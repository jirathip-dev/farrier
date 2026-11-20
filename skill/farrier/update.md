# farrier-update

`Updater` finds the release of a branch's head, verifies it, swaps it in with one rename, restarts your daemon
through `Daemon`, and puts the old binary back if the new one does not serve. It prints, logs and alerts
nothing: every call ends in an `Outcome` (or an `Error` when the pass could not run at all), and your tool says
what it means. Worked code: [a daemon](examples/daemon.md), [a plain CLI](examples/cli.md),
[a rollout over several hosts](examples/rollout.md).

## What you give it

- `Product { bin, repo, keys, codesign_id }`: the asset name `<bin>-<triple>`, `<owner>/<repo>`, the minisign
  public keys you trust (`include_str!` your `release.pub` and `release-backup.pub`), and the macOS code-signing
  identifier (`None` skips the code-signature check).
- `Install { exe, state }`: the live executable and a folder of its own. The staged file is `<exe>.new`, the
  previous build `<exe>.prev` (one generation, a hard link taken just before the swap). `state` holds
  `state.json`, `lock` and `paused`; nothing else.
- `Channel::Main` or `Channel::Branch(name)`: the branch whose head you follow, and so the tag (`build-<sha7>` or
  `build-<branch>-<sha7>`).
- `Limits { download, probe, confirm }` (default 300 s, 10 s, 60 s): a download, the staged binary's `--version`,
  and how long the new build has to serve before it is rolled back.
- A `Daemon` (`.daemon(&d)`), the consumer's whole part:
  - `admit(&Staged)`: may this build take over now; `Err(why)` defers it (default: yes).
  - `restart(&Build)`: put the swapped-in build in service; `Restarted::Now`, or `Restarted::Later(why)` when a
    later pass should ask again.
  - `serves(&Build)`: does that build answer now. Asked until `confirm` passes.
  - `Supervised::new(&service, probe)` is the ready one: restart a `farrier_service::Service`, then ask `probe`.
    `NoDaemon` (the default) is a plain CLI: the new file is the update.

Hosts need `git` and `curl` on `PATH`; macOS needs `codesign`.

## A pass

`run()` is the whole chart: paused, a swap not yet confirmed, the head by `git ls-remote`, then the release by
the tag built from that sha, verified in the contract's order ([contract.md](contract.md), checks 2 to 6). Any
failure removes `<exe>.new` and leaves the live binary as it was. Then `admit`, the one rename over the live
file, `restart`, `serves` within `confirm`, else `.prev` goes back, the old build is restarted, and the new one
is marked failed: later passes skip it until the head moves. A `Signature`, `Checksum`, `CodeSign` or `Version`
reject marks the build failed too; a `Download` reject is tried again next pass (`Reject::retry()`).

| call | does |
|---|---|
| `run()` | the whole chart; honours `pause` |
| `plan()` | what `run` would do, read-only, wrapped in `Would` |
| `stage(&build)` | download and verify into `<exe>.new`, and rest there |
| `swap(&build)` | from a staged `build`: admit, swap, restart, confirm or roll back; refuses any other build |
| `pause()`, `resume()` | stop and restart `run` only; `stage` and `swap` stay explicit commands |
| `builds()` | `Builds { live, staged }` |
| `status()` | `state.json` as `State { at, resting, failed, last, ok }` |

`Staged` and `Swapped` rest on disk, so a pass that dies resumes where it stopped and a coordinator may leave a
host staged as long as it likes. `fetch(product, channel, build, target, to)` verifies one build for a target
into a path: a new host's first binary.

## Outcomes

Print `outcome` as it is: its `Display` is the one line. `outcome.exit()` is nonzero for `Rejected`,
`RolledBack` and `Skipped`.

| outcome | line |
|---|---|
| `Paused { since }` | `paused since <unix secs>` |
| `Resumed` | `resumed` |
| `Current(build)` | `current: <build>` |
| `Waiting { tag, why }` | `waiting for <tag>: <why>`: no head, or no `SHA256SUMS` for it yet |
| `Skipped { build, why }` | `skipped <build>: <why>`: it failed here before |
| `Rejected(reject)` | `rejected: <Kind>: <why>`: `NoAsset`, `Download`, `Signature`, `Checksum`, `CodeSign`, `Version` |
| `Staged(build)` | `staged <build>` |
| `Deferred { build, why }` | `deferred <build>: <why>`: `admit` said not yet |
| `Pending { build, why }` | `pending <build>: <why>`: `restart` said later |
| `Updated { from, to }` | `updated <from> -> <to>` (`a dev build` when the live binary named none) |
| `RolledBack { to, why, serving }` | `rolled back <to>: <why>; the previous build serves` (or `does not serve`) |
| `Would(outcome)` | `would: <line>` |

## Phases, for a rollout over several hosts

The rollout (who waits for whom, how long) stays in your tool; farrier offers the two steps. Give your binary a
hidden verb that prints `phase::run(&updater, phase, &build)`: one JSON line with the outcome and the host's
`Builds`. The coordinator runs it on each host over its own transport and reads the line with `phase::parse`,
which returns `Reply { outcome, builds }`, or the other host's `Error`. `Phase::Builds` only reports.
