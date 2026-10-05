# 00 — Overview

SPL-ASDLC is a development process for repositories where agents write code, together with the documentation that explains it and the tooling that applies it. This file states the invariants every other file refines, names the actors, and fixes the three moments of quality assurance.

## Invariants

1. **The contract comes before the implementation.** A ticket is implementable only when it carries a goal, non-goals, acceptance criteria and business rules, and that contract has been verified by refinement. The release-note snippet is written at refinement time. Nothing without a contract is taken.
2. **An implementation ends in a pull request, and the acceptance criteria are ticked as they are met.** Several pull requests per ticket are allowed; one is preferred.
3. **Business rules are global and permanent.** They apply to the whole current state of the project, not to the diff; they live in the repository; each one has a deterministic test in CI. A rule without a test is only believed to be respected.
4. **PR review is fast and on the diff.** CI first (build, tests, lint; the content is the project's). Then several reviewers, each with its own angle, configurable per project. Then one consolidator who applies the retained suggestions. Then CI again, then merge, then "To Deploy".
5. **Release QA is exhaustive and on the whole project.** A release branch is cut from `main`; CI is green on it; QA angles review the entire candidate, not the diff; each finding is blocking or deferred; each fixed finding adds a deterministic test; each loop is a release candidate (RC1, RC2, …, the count is configurable); the consolidator validates the findings and has the last word; the GO is computed, or given by a human where the project says so.
6. **Deployment is the project's.** Closure re-verifies every shipped ticket (all boxes ticked → Done, otherwise back to In Progress, or Post Deploy when only post-deploy boxes remain). The release note concatenates what was actually delivered, and sending it is part of the deliverable.
7. **Agnostic to the agent runtime.** Agents may run on a fleet, in an interactive session, in a cloud orchestrator or by hand. The process and the tooling are the same; the harness is instance configuration.
8. **Agnostic to the tracker.** The tracker lives behind an interface. Linear and Livela are the first adapters.

Two consequences follow from how the tooling is built, and they are invariants too:

9. **CI only publishes facts to GitHub.** A workflow creates branches, tags, check results, artifacts, labels and merges. It never writes to a tracker and never needs anything outside GitHub.
10. **One deterministic process, the bridge, writes every state transition.** It runs in the user's environment, reads GitHub, writes to the tracker, computes the GO and merges what the method says to merge. An agent never changes a status; an agent writes code, report files, review comments and ticks its own acceptance criteria.

## Actors

| Actor | What it does | Who it is |
|---|---|---|
| **Implementer** | Takes a ticket with a contract, works on a branch, ticks acceptance criteria, opens the PR. | An agent of the runtime, or a human. |
| **Reviewer (angle)** | Reads a diff (PR review) or a whole candidate (release QA) from one angle: security, design, product, maintainability, … Posts findings. | An agent or a human; several in parallel. |
| **Coordinator** | Reads every angle's findings, discards false alarms with a reason, confirms severities, applies the retained fixes, approves. Has the last word of a QA. | An agent or a human; one per PR or per release candidate. |
| **The bridge** | Deterministic code. Creates the items, posts the brief, merges on approval, computes the GO, closes the release, sends the release note. | The `spl-asdlc` binary or library, hosted by the user's environment. |
| **The human** | Cuts a release, requalifies a finding, gives the GO where the project's policy says so, takes an escalation. | The project owner. |

## The three moments of QA

The same shape at three moments, with the scope widening each time:

| Moment | Object | Scope | Who | Output |
|---|---|---|---|---|
| Refinement | The ticket | The contract | A refiner and the owner | A ticket that can be taken |
| PR review | The diff | What the PR changes | Angles + coordinator | Findings in the PR, fixes on the branch, approval |
| Release QA | The candidate | The whole project | Angles on branches + coordinator | Angle reports and fixes merged into the release branch, a computed GO |

Refinement is QA applied to the ticket before the implementation; release QA is QA applied to the code after it. Same principle, different moments.

## What is fixed and what is configurable

Fixed by the method: the contract, the registry of business rules with tests, the existence of distinct angles with distinct contexts, the consolidator, the computed GO and its inputs, the closure rules, the vocabulary, the fact that the bridge alone writes states. Configurable per project: the CI content, the angles and who takes them, the number of RC passes, the GO policy (computed or human), the merge policy, the release shape, the notification channels, the cadence of the bridge.
