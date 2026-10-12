# SPL-ASDLC

**ASDLC by Side Projects Lab.** An agentic software development life cycle: a defined process, the documentation that explains it, and the executable tooling that applies it to any repository where agents write code.

SPL-ASDLC is agnostic to the agent runtime (AgentForge, a Claude Code session, Codex, anything that can read a ticket and open a pull request) and to the tracker (Linear and Livela first, Jira later). The core is the CI and QA tooling: ticket contracts, business rules with deterministic tests, multi-angle PR review with a consolidator, release candidates reviewed by parallel QA angles, a computed GO, closure that re-verifies every shipped ticket, and a release note that is sent as part of the delivery.

Each of these layers is optional, chosen in `spl-asdlc.toml`: the PR review, the release cycle (`[release]`), its QA angles and the sensitive paths (`docs/setup/README.md`, "Start light, add layers"). What every project runs is the ticket contract, the business-rule registry with its tests, CI, the bridge, and CI that only publishes facts.

## The three parts

1. **The method** — `docs/spec/`. What a ticket must contain before anyone takes it, how a PR is reviewed and merged, how a release is cut, reviewed, gated and closed, and the vocabulary everything else uses.
2. **Agent protocols** — `docs/agents/`. Runtime-agnostic Markdown that any agent can load as a skill: review a PR, review a release on an angle branch, consolidate findings, refine a ticket. The bridge embeds the protocol of each review and consolidation item in the item it serves (BR-ASDLC-015).
3. **The tooling** — the Rust crate `spl-asdlc` (library and `spl-asdlc` binary) and the reusable GitHub Actions workflows `.github/workflows/asdlc-*.yml` with the composite action `actions/required-check`, which a project calls from its own workflows (`docs/setup/examples/`). The crate is "the bridge": one deterministic process that runs in the user's environment, polls GitHub, writes state transitions to the tracker, computes the GO and merges what must be merged. CI workflows only publish GitHub facts; they never write to a tracker. An implementer or a reviewer never changes a status; the bridge does (refinement apart, which sets a verified ticket Ready), except on an item a person took over with the hands-off label (`docs/spec/07-the-bridge.md`, "Hands off").

## Status

In use. The specification and the requirements are written (`docs/requirements/` is derived from the Python implementation this product replaces). The bridge is released: v0.1.0 to v0.3.0 are published, with their binaries, and a sandbox repository runs the flow end to end. `cargo test` runs the suite, `spl-asdlc config check` validates a `spl-asdlc.toml`. The repository is private until the public switch; it then becomes public, here and as the `spl-asdlc` crate on crates.io. It is published under the Apache License 2.0 (see `LICENSE`).

## Layout

| Path | Content |
|---|---|
| `docs/spec/` | The method, one file per topic |
| `docs/agents/` | Agent protocols (skills) |
| `docs/requirements/` | Requirements for the crate and the workflows, with the test cases to carry over |
| `docs/setup/` | How to configure a repository, a GitHub App and a tracker identity for SPL-ASDLC |
| `docs/business-rules.md` | The product's own business rules, each with its test |
| `docs/livela-contract.md` | What the Livela tracker must offer for its adapter |
| `scripts/` | Checks that run in CI (no instance names, …) |
| `.github/workflows/asdlc-*.yml`, `actions/` | The reusable workflows and the composite action a project calls |
| `.github/workflows/` (the others) | This repository's own CI: `pr.yml` and `pr-title.yml` call this commit's PR-side `asdlc-*` workflows; `release-please.yml` keeps the release PR on `main`; `publish.yml` calls `binaries.yml`, which builds and attaches the binaries of a release |

## How spl-asdlc itself is developed

This repository does not run the SPL-ASDLC flow on itself. It follows GitHub Flow: every change is a pull request to `main`, checked by `pr.yml` and `pr-title.yml`. Releases are release-please's normal mode on `main`: it keeps a release PR open, and merging it tags `v<X.Y.Z>` and publishes the GitHub release, whose binaries `publish.yml` builds and attaches. The method (the bridge, the `asdlc-*` workflows, the release cycle) is exercised on a separate sandbox repository, against a staging channel of the bridge, before each release (`docs/setup/bridge.md`, "Channels"), so that a bug in the flow never blocks its own fix.

## Rule zero

This repository carries no name of any product, agent, person or installation that uses the method. Integrations the method supports (Linear, Livela, Jira, AgentForge, Claude Code, Codex, GitHub) are named; installations are not. `scripts/check-no-instance-names.sh` enforces it (business rule BR-ASDLC-001).
