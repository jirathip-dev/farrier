# The release contract

What a release must look like for farrier-update to install it. gatekeeper's release workflow writes it;
`farrier release verify` checks it; farrier-update reads it. This page is the one text all three follow.

It is today's layout in cooee, drover and shed plus one new file, the signature. So a host crosses over through the
updater it already has, and no host needs a manual step.

## The release

| | rule |
|---|---|
| When | one release per commit, after CI ends green on a push to a release branch |
| Tag on `main` | `build-<sha7>`; a normal release, kept |
| Tag on another branch | `build-<branch>-<sha7>`; a prerelease, pruned by the workflow after 14 days |
| Target | the commit; `<sha7>` is its first 7 hex characters |
| Never | an asset changed or added after the release is published |

## The assets

| asset | rule |
|---|---|
| `<bin>-<triple>` | the executable itself, not an archive. Triples: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`; `x86_64-apple-darwin` and `aarch64-unknown-linux-gnu` when a repo builds them. A release may hold several `<bin>` |
| `SHA256SUMS` | one line per executable: 64 lowercase hex, two spaces, the asset name. Nothing else |
| `SHA256SUMS.minisig` | a minisign signature of `SHA256SUMS`, with the trusted comment `<owner>/<repo> <tag> <sha40>` |

- A triple may be missing (one platform's build or signing failed). `SHA256SUMS` lists exactly what is there; a host
  of the missing triple reports `no asset` and waits for the next head.
- macOS executables are signed with a Developer ID Application certificate, identifier `<bin>`, hardened runtime,
  and notarized.
- `<bin> --version` ends its last line with `(<hex>)`, 7 to 40 hex characters that are a prefix of the commit. How
  the build learns its commit stays the consumer's (each has a build-time variable today).

## What farrier-update checks, in order

1. The head: `git ls-remote https://github.com/<repo> refs/heads/<branch>`. No API quota, no token, and a release
   published out of order cannot step a host back.
2. `SHA256SUMS` and `SHA256SUMS.minisig` of the head's tag. Neither there: `Waiting`.
3. The signature, against one of the public keys compiled into the running binary (`Product.keys`), and the trusted
   comment against this repo, tag and head. A copy of another release's signed list fails here.
4. The asset's SHA-256, computed in process, against its line.
5. On macOS: `codesign -v -R` with the Developer ID requirement for `codesign_id`.
6. The staged file's `--version` names the head.

Any failure removes the staged file and leaves the live binary as it was.

## Keys

- One signing key for the fleet's releases, held as a secret of the workflow's `release` environment beside the
  code-signing secrets. Its public half is a file in each consumer repo (`release.pub`), compiled in.
- `Product.keys` is a list, so a key is rotated in two releases: one that trusts both keys, then one signed with
  the new key.

## The checker

`farrier release verify <dir> --repo <owner>/<repo> --tag <tag> --sha <sha40> --bin <bin> --key <release.pub>`
runs every check the folder allows and exits nonzero on the first failure: 2 to 4 once `SHA256SUMS` is there, 5 and
6 for the assets of the runner's own triple. gatekeeper's workflow runs it twice: in each build job on that job's
asset, and in the publish job after signing. A release that farrier would reject is never published.
