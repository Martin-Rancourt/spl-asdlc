# SPL-ASDLC

**ASDLC by Side Projects Lab.** An agentic software development life cycle: a defined process, the documentation that explains it, and the executable tooling that applies it to any repository where agents write code.

SPL-ASDLC is agnostic to the agent runtime (AgentForge, a Claude Code session, Codex, anything that can read a ticket and open a pull request) and to the tracker (Linear and Livela first, Jira later). The core is the CI and QA tooling: ticket contracts, business rules with deterministic tests, multi-angle PR review with a consolidator, release candidates reviewed by parallel QA angles, a computed GO, closure that re-verifies every shipped ticket, and a release note that is sent as part of the delivery.

## The three layers

1. **The method** — `docs/spec/`. What a ticket must contain before anyone takes it, how a PR is reviewed and merged, how a release is cut, reviewed, gated and closed, and the vocabulary everything else uses.
2. **Agent protocols** — `docs/agents/`. Runtime-agnostic Markdown that any agent can load as a skill: review a PR, review a release on an angle branch, consolidate findings, refine a ticket.
3. **The tooling** — the Rust crate `spl-asdlc` (library and `spl-asdlc` binary) and the reusable GitHub Actions workflows `spl-asdlc-ci`. The crate is "the bridge": one deterministic process that runs in the user's environment, polls GitHub, writes state transitions to the tracker, computes the GO and merges what must be merged. CI workflows only publish GitHub facts; they never write to a tracker. Agents never change a status; the bridge does.

## Status

First draft. The specification and the requirements are written (`docs/requirements/` is derived from the Python implementation this product replaces). The crate is being built lot by lot: `cargo test` runs the suite, `spl-asdlc config check` validates a `spl-asdlc.toml`. The repository is private until the pilot on Linear is green (step 4a of the plan); it then becomes public, here and as the `spl-asdlc` crate on crates.io, before the Livela pilot, which depends on the published crate. It is published under the Apache License 2.0 (see `LICENSE`).

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

## Rule zero

This repository carries no name of any product, agent, person or installation that uses the method. Integrations the method supports (Linear, Livela, Jira, AgentForge, Claude Code, Codex, GitHub) are named; installations are not. `scripts/check-no-instance-names.sh` enforces it (business rule BR-ASDLC-001).
