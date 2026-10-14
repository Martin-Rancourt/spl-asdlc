# SPL-ASDLC

**ASDLC by Side Projects Lab.** A delivery method for software written by agents, and the tooling that applies it to any repository: ticket contracts, business rules proven by tests, optional review by angle, a release cycle with a computed GO, and a bridge that keeps the tracker and GitHub in step.

It works with any agent runtime that can read a ticket and open a pull request (AgentForge, Claude Code, Codex…) and with Linear or Livela as the tracker.

**Start with [`docs/method.md`](docs/method.md): the method in ten minutes.**

## How the documentation is organised

| Where | For whom | What |
|---|---|---|
| [`docs/method.md`](docs/method.md) | Anyone deciding whether to use the method | The method, authoritative |
| [`docs/setup/`](docs/setup/) | A project adopting it | Setting a repository up: the GitHub App, the rulesets, the workflow callers (`examples/`), the configuration template (`templates/`), the tracker, the bridge's host, and the commands to take any step over by hand (`manual-gestures.md`) |
| [`docs/agents/`](docs/agents/) | The agents | The protocols an agent loads to refine a ticket, implement it, review a PR or a release, or consolidate findings |
| [`docs/reference/`](docs/reference/) | Whoever implements or operates the bridge | The detailed rules behind `method.md`, one file per topic, with every edge case |
| [`.spl-asdlc/`](.spl-asdlc/) | The people developing spl-asdlc | This repository's own internal documentation: its business rules (one file per rule), the requirements it was built from, the open points, how spl-asdlc itself is developed and tested, and its QA records |

`.spl-asdlc/` is also the one folder the method asks of every project that uses it: its configuration (`config.toml`), its business rules and its QA reports live there, and the rest of the repository is organised as the project likes. spl-asdlc follows the same convention for its own internal documentation.

## The tooling

- **The bridge**: the Rust crate `spl-asdlc` and its binary. It runs at a regular interval on a host the project chooses, reads GitHub facts, moves the tickets, merges what may be merged, and drives the release.
- **The reusable workflows**: `.github/workflows/asdlc-*.yml` and the composite action `actions/required-check`, which a project calls from its own workflows. They only publish facts and never write to a tracker.

The other workflows in `.github/workflows/` are this repository's own CI. spl-asdlc does not run the method on itself; `.spl-asdlc/development.md` explains how it is developed and released.

## Status

In use. The bridge is released, with binaries for Linux, macOS and Windows. The repository is private until its public switch; it is published under the Apache License 2.0 (see `LICENSE`).

## Rule zero

This repository carries no name of any product, agent, person or installation that uses the method. The integrations it supports (Linear, Livela, Jira, AgentForge, Claude Code, Codex, GitHub) are named; installations are not. `scripts/check-no-instance-names.sh` enforces it.
