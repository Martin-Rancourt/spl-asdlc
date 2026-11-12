# BR-ASDLC-023 — spl-asdlc runs its own method at a published, pinned version, never at the commit being changed, so that a bug of the flow never checks, merges or releases its own fix.

Status: Enforced

## Rule

This repository is a project of the method like any other: its tickets live in the tracker, the bridge merges its PRs, and it releases through the method's release cycle (cut, gate, publication). Every call its workflows make to a method unit (an `asdlc-*` workflow, an action of `actions/`) names this product's repository at one published minor line `@v<X.Y>`, the line the bridge that serves it embeds, and the cut runs the published binary of that same line; never `./`, a branch, or `main`. It carries `.spl-asdlc/config.toml` and the process page of that version at `.spl-asdlc/process.md`, checked as in any project. The line moves only in a PR of its own, once the bridge that serves this repository embeds the new line. A manual merge on GitHub stays the fallback when the pinned version cannot serve its own fix (`.spl-asdlc/development.md`, "When the method breaks").

## Evidence

The owner's decision of 2026-10-18 (SID-1191) kept this repository outside its method, so that a bug of the flow could not block the release that fixes it. The owner's decision of 2026-11-12 (ASDLC-VSYK) brought it in, with the full cycle: outside the method, tickets whose PR only waited for a human merge were served to agents again and again, and nothing merged them. Pinning a published version keeps the loop open: the code being changed never runs the flow that judges it.

## Test

`tests/workflows/own.rs`: `this_repository_runs_the_method_at_one_pinned_line`, `this_repositorys_workflows_are_its_callers_and_binaries`, `every_call_is_to_this_commit_with_declared_inputs_and_secrets`
