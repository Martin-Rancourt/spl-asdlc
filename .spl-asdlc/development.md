# How spl-asdlc itself is developed

This repository runs the SPL-ASDLC method on itself, like any project, at a published, pinned version: the bridge that serves it embeds a released `spl-asdlc`, and every workflow caller names that release's minor line (`@v1.9` today), never this commit nor `main` (BR-ASDLC-023). The procedure is the method's: `.spl-asdlc/process.md`, around which `AGENTS.md` adds this repository's own instructions.

## Changes

Every change is a pull request to `main`, or a fix to the release branch in flight, checked by `pr.yml` (the method's checks in one job of the pinned `asdlc-pr-checks.yml`, Conventional Commits title included; format, lints, tests, rule zero). The bridge squash-merges it once green; no PR review and no QA angle are configured (`.spl-asdlc/config.toml`). The workflows a PR changes are not the ones that check it: the tests (`tests/workflows/`) check them, and the sandbox runs them ("Testing the method").

## Releases

The method's release cycle (`.spl-asdlc/process.md`, "The release cycle"): `release-cut.yml` opens the release branch and its first candidate `v<X.Y.Z>-rc.<N>` (the version in `Cargo.toml` and `Cargo.lock`, the CHANGELOG section, from the commits since the last release); `release-gate.yml` runs `pr.yml`'s checks on the candidate; the bridge computes the GO and publishes the GitHub release `v<X.Y.Z>`; `release-publish.yml` verifies it, builds the binaries of every target (`binaries.yml`) and attaches them with their `SHA256SUMS`, moves the minor tag `v<X.Y>` to the release, publishes the crate on crates.io (the `crates-io` job), and merges the release branch back into `main`. The reusable `asdlc-*` workflows are released with the bridge under the same version; callers pin a major tag (`@v1`), moved by the owner, or a minor line (`@v<X.Y>`), moved by the publication. Releases until 1.10.0 were release-please's.

Publishing a release is still the owner's go: the bridges of real projects install it within minutes. A person dispatches the cut; nothing else is done by hand.

## Updating the bridge that serves this repository

A release changes the bridge for every project, this one included, but this repository keeps running the line it pins until both of these moved, in this order:
1. **The bridge.** The host that runs the bridge for this repository embeds `spl-asdlc` by version (the Livela runner, `docs/reference/livela-contract.md`): its owner bumps that dependency to the new line in a PR of the host, and redeploys the runner. Until then the bridge serving this repository is the previous line.
2. **The pin.** Then a PR of this repository moves every `@v<X.Y>` of `.github/workflows/` and the cut's `spl-asdlc-version` to the new line, and refreshes `.spl-asdlc/process.md` from the release's `docs/agents/process.md` (`tests/workflows/own.rs` keeps them one line). Never before step 1: the workflows would then expect a bridge that is not running.

A configuration key the new line adds enters `.spl-asdlc/config.toml` only after step 1.

## When the method breaks

If the pinned version cannot serve a fix of its own (the bridge stopped on this repository, a check refuses every PR), the fallback is a manual merge on GitHub: the owner merges the fix's PR by hand, and, when a release is needed, dispatches the cut, or tags and publishes the release by hand (`docs/setup/manual-gestures.md`). The bridge takes each gesture as done. Then fix forward through the cycle.

**The crate on crates.io**: the `crates-io` job of `publish.yml` runs `cargo publish --locked` from the release's tag, once the binaries are built and tested, and publishes nothing for a version crates.io already holds or a tag that is not `v<X.Y.Z>`. It reads its token from the GitHub environment `crates-io`, set up once by the owner:
- on crates.io (Account settings → API Tokens), a token scoped to the crate `spl-asdlc` with the `publish-update` scope only, and an expiry the owner chooses;
- in the repository (Settings → Environments), the environment `crates-io`, its deployment rule limited to the tags `v*.*.*` ("Selected branches and tags", a tag rule), and in it the environment secret `CARGO_REGISTRY_TOKEN` holding that token.

Without the secret the job fails and says so; the rest of the publication is done. A version that missed crates.io is published by rerunning the `crates-io` job of its `publish` run, once the secret is set. 1.3.2 was published by hand; 1.3.3 and 1.4.0 are not on crates.io.

The process page, `docs/agents/process.md`, is the source of the copy every project carries (this one too, at `.spl-asdlc/process.md`, refreshed only when the pin moves); it is copied into every project and checked there by `asdlc-pr-checks.yml` and `asdlc-pr-guards.yml` against the git blob id each workflow carries (`PINNED_BLOB`). A PR that changes the page writes the new id in both, in the same PR (`tests/workflows/pr_guards.rs` names it, `pr_checks.rs` keeps the two equal), and its release note tells the projects to refresh their copy: once `v1` moves to that release, a project's PRs are refused until it does.

## Tickets

Its tickets live in Livela, in the SPL-ASDLC project (prefix `ASDLC`), linked to this repository; the bridge moves them as in any project (`.spl-asdlc/process.md`, "The ticket's statuses"): In Review when the PR opens, To Deploy at its merge, Done at the closure of the release that ships it. Until 2026-10-24 they lived in Linear's ASDLC project, now read-only: each of its tickets links to its Livela copy.

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
