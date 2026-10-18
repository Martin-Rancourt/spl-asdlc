# How spl-asdlc itself is developed

This repository does not run the SPL-ASDLC method on itself, so that a bug in the flow never blocks its own fix.

## Changes

GitHub Flow: every change is a pull request to `main`, checked by `pr.yml` (format, lints, tests on Linux, macOS and Windows, rule zero) and `pr-title.yml` (Conventional Commits). It is squash-merged once green.

## Releases

release-please's normal mode on `main` (`release-please.yml`): it keeps a release PR open, and merging it tags `v<X.Y.Z>` and publishes the GitHub release. `publish.yml` then builds the binaries (`binaries.yml`) and attaches them with their `SHA256SUMS`. The reusable `asdlc-*` workflows are released with the bridge under the same version; callers pin a major tag (`@v1`).

## Testing the method

The method (the bridge, the `asdlc-*` workflows, the release cycle) is exercised on a separate sandbox repository before each release, against the **staging channel** of the bridge:
- `staging.yml` rebuilds the rolling prerelease `staging` on every push to `main`: the archive `spl-asdlc-staging-x86_64-unknown-linux-gnu.tar.gz`, its `SHA256SUMS`, and a `COMMIT` file in the archive naming the commit built. The `staging` tag is moved to that commit.
- The sandbox's bridge host installs a staging build whenever its commit differs from the installed one: staging has no version order. A flow bug is caught on the sandbox before a release, and its fix reaches the sandbox as soon as it is merged.
- The sandbox's cut caller names the channel with `spl-asdlc-version: staging`, so the cut runs the same build as its bridge.
- Never put a real project on `staging`: real projects follow the published releases (`docs/setup/bridge.md`).

## This folder

- `business-rules/`: this repository's own business rules, one file per rule, with the tests that prove them.
- `requirements/`: the requirements the method was extracted from, with dated correction notes where the method later changed.
- `open-points.md`: the questions still open.
- `qa/`: QA records.
- `brand.md`: the logo, what it means, its colors, which file in `assets/brand/` to use where, and how to regenerate it.
