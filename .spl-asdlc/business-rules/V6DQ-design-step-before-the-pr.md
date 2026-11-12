# V6DQ — A ticket that needs design is implemented in two passes, and its PR opens only once the person's design task is closed.

Status: Proposed

## Rule

A ticket labelled `angle/design` is implemented in two passes: first the back end and a first draft of the interface on a plain branch, with no PR; then a design task, a child of the ticket assigned to the person who designs, which blocks the ticket while it stays In Progress. Its PR opens only once that task is closed, after the branch is brought up to date with the base. On the PR, the `design` angle is reviewed by the designer agent: no design review task goes to a person on a ready PR.

## Evidence

Owner's decisions of 2026-11-12, ASDLC-VKWN: a PR that waited on a person's design review went back to draft each time the base moved, and three review rounds were opened for one ticket, none consolidated.

## Test

No executable enforcement claimed yet: the rule is the agents' procedure (`docs/agents/process.md`, "The design step") and the design angle's assignee in each project's configuration.
