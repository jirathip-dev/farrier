# The release contract

What a release must hold for `farrier-update` to install it, and how to check one with `farrier release verify`.

## A release

- One release per commit, after CI ends green on a push to a release branch. Tag `build-<sha7>` on `main` (a
  normal release), `build-<branch>-<sha7>` on another branch (a prerelease, pruned after 14 days). Its target is
  the commit.
- A release may carry a tag its caller names (`v<version>`), for a tool consumers pin by version. Assets,
  `SHA256SUMS`, the signature and its trusted comment `<owner>/<repo> <tag> <sha40>` are unchanged. `farrier-update`
  still finds a daemon's release by the build tag of its branch head.
- Assets, never changed or added after the release is published:
  - `<bin>-<triple>`: the executable itself, not an archive. Triples `aarch64-apple-darwin`,
    `x86_64-unknown-linux-gnu`, and `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu` when you build them. A
    triple may be missing.
  - `SHA256SUMS`: one line per executable there, `<64 lowercase hex>  <asset>` (two spaces), each ending in a
    newline. Nothing else: no blank line, no comment, no `*`, no path.
  - `SHA256SUMS.minisig`: a minisign signature of `SHA256SUMS` whose trusted comment is
    `<owner>/<repo> <tag> <sha40>`.
- macOS executables are signed with a Developer ID Application certificate, identifier `<bin>`, hardened runtime,
  and notarized.
- `<bin> --version` ends its last line with `(<hex>)`: 7 to 40 hex characters that are a prefix of the commit.

## Keys

Compile two minisign public keys into your daemon from its first signed release: `release.pub` (its secret half
is a secret of your release workflow's `release` environment) and `release-backup.pub` (its secret half stays
offline). A signature by either passes.

## Check a release before you publish it

```sh
farrier release verify <dir> --repo <owner>/<repo> --tag <tag> --sha <sha40> --bin <bin> \
  --key release.pub --key release-backup.pub
```

It runs, in order and as far as `<dir>` allows, and exits nonzero on the first failure, naming the `Reject`:

| check | when | `Reject` on failure |
|---|---|---|
| the signature, by any `--key`, and its trusted comment names this repo, tag and sha | `SHA256SUMS` is there | `Signature` |
| `SHA256SUMS` is strict, and lists every `<bin>-<triple>` in `<dir>` | `SHA256SUMS` is there | `Checksum` |
| each listed asset is in `<dir>` with its SHA-256 | `SHA256SUMS` is there | `NoAsset`, `Checksum` |
| `codesign` with the Developer ID requirement | macOS, this host's asset is there | `CodeSign` |
| `--version` names the sha | this host's asset is there | `Version` |

Run it in each build job on that job's asset, then in the publish job after signing: a release it rejects is
never published.

## In code

`farrier_update::contract` (`Sums`, `verify_sums`, `version`, `Reject`) and `farrier_update::id` (`Build`,
`Channel::tag`, `Target::asset`) hold the same rules, pure and without I/O.
