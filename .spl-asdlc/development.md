# How spl-asdlc itself is developed

This repository does not run the SPL-ASDLC method on itself, so that a bug in the flow never blocks its own fix (BR-ASDLC-023).

## Changes

GitHub Flow: every change is a pull request to `main`, checked by `pr.yml` (format, lints, tests on Linux, macOS and Windows, rule zero) and `pr-title.yml` (Conventional Commits). It is squash-merged once green.

## Releases

release-please's normal mode on `main` (`release-please.yml`): it keeps a release PR open, and merging it tags `v<X.Y.Z>` and publishes the GitHub release. `publish.yml` then builds the binaries (`binaries.yml`), attaches them with their `SHA256SUMS`, moves the minor tag `v<X.Y>` to the release, and publishes the crate on crates.io (the `crates-io` job). The reusable `asdlc-*` workflows are released with the bridge under the same version; callers pin a major tag (`@v1`), moved by the owner, or a minor line (`@v<X.Y>`), moved by `publish.yml`.


**The crate on crates.io**: the `crates-io` job of `publish.yml` runs `cargo publish --locked` from the release's tag, once the binaries are built and tested, and publishes nothing for a version crates.io already holds or a tag that is not `v<X.Y.Z>`. It reads its token from the GitHub environment `crates-io`, set up once by the owner:
- on crates.io (Account settings → API Tokens), a token scoped to the crate `spl-asdlc` with the `publish-update` scope only, and an expiry the owner chooses;
- in the repository (Settings → Environments), the environment `crates-io`, its deployment rule limited to the tags `v*.*.*` ("Selected branches and tags", a tag rule), and in it the environment secret `CARGO_REGISTRY_TOKEN` holding that token.

Without the secret the job fails and says so; the rest of the publication is done. A version that missed crates.io is published by rerunning the `crates-io` job of its `publish` run, once the secret is set. 1.3.2 was published by hand; 1.3.3 and 1.4.0 are not on crates.io.

The process page, `docs/agents/process.md`, is copied into every project and checked there by `asdlc-pr-guards.yml` against the git blob id the workflow carries (`PINNED_BLOB`). A PR that changes the page writes the new id in the same PR (`tests/workflows/pr_guards.rs` names it), and its release note tells the projects to refresh their copy: once `v1` moves to that release, a project's PRs are refused until it does.

## Tickets

This repository is the one project outside its own method (BR-ASDLC-023): no bridge moves its tickets, and it has no release cycle for them to follow. Its tickets live in Livela, in the SPL-ASDLC project (prefix `ASDLC`); the session that does a step moves the ticket. Until 2026-10-24 they lived in Linear's ASDLC project, now read-only: each of its tickets links to its Livela copy.
- **In Progress** when the work starts, **In Review** when its PR is opened.
- **Done** when its PR is merged into `main`, every box the PR delivers ticked in the ticket itself. To Deploy is not used here. A ticket whose work landed through another ticket's PR is closed with it, the PR linked.
- Publishing a release (merging the release-please PR, moving the major tag) moves no ticket. It is a gesture of the owner's: a session does it only on the owner's explicit go for that release.

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
