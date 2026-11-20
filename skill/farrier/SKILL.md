---
name: farrier
description: Install a daemon as a launchd or systemd job and update it from its verified GitHub releases with the farrier crates. Use when a Rust daemon or CLI installs, restarts or removes its own job, or replaces its own binary from a release.
---

# farrier

Two library crates and one tool. Depend on them by git, pinned by `rev`, and list the repo under `allow-git` in
your `deny.toml`.

| you need to | use |
|---|---|
| write, load, restart or remove a per-user launchd or systemd job | `farrier-service` |
| find, verify, swap in and roll back a release of your own binary | `farrier-update` (it restarts through `farrier-service`) |
| copy this skill into a repo | `farrier skill --to <dir>`, which writes `<dir>/farrier/` |
| check a release folder before it is published | `farrier release verify` |

## Pages

- [service.md](service.md): each `Job` field, the plist and unit keys it writes, and the verbs.
- [update.md](update.md): `Daemon`, every `Outcome` line, the state files, the phases.
- [contract.md](contract.md): the release layout and `farrier release verify`.
- `examples/`: [a daemon](examples/daemon.md), [a plain CLI](examples/cli.md), [a rollout](examples/rollout.md).

## Rules for callers

- Use the crates' public items only; never start `launchctl`, `systemctl`, `curl` or `git` yourself for a job or an
  update.
- The crates print, log and alert nothing: they return an outcome, and your daemon says what it means.
- Rerun `farrier skill --to <dir>` after you bump the pinned `rev`, so this page matches the code you build.
