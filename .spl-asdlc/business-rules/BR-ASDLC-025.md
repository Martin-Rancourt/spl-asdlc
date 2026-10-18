# BR-ASDLC-025 — Agents' process instructions come from ASDLC, whatever the project; a project outside ASDLC keeps its own in its own repository, never in ASDLC.

Status: Proposed

## Rule

In a project that runs the method, an agent knows how to work (where to branch, how a PR is titled and merged, how its ticket moves, who closes it and when, what it never does by hand) from the method's process page alone, `docs/agents/process.md`, copied into the project (BR-ASDLC-026). No project and no agent runtime carries its own copy of these instructions. A project outside the method keeps its own process instructions in its own repository, never in the method. The one such project is `spl-asdlc` itself, whose instructions are `.spl-asdlc/development.md` and `CLAUDE.md`.

## Evidence

The owner's decision of 2026-10-18 (SID-1193): a cleanup found tickets left in To Deploy because agents did not know which flow applied to which project, their process rules being injected by the runtime from notes of its own.

## Test

None that runs in CI yet: what a runtime injects is outside this repository. The method's side, the page and the agent file that points to it, is held by BR-ASDLC-026 and BR-ASDLC-028.
