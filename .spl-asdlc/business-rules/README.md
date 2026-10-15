# SPL-ASDLC business rules

One file per rule, `<ID>.md`, in the format of `docs/spec/02-business-rules.md`. Ids are `BR-ASDLC-NNN` and are never reused. A rule is `Enforced` only when the test its file names runs in CI; until then it is `Proposed`.

This page is an index for people, kept by hand; the method never reads it, and a test checks that it lists every rule file and nothing else.

| ID | Rule | Status |
|---|---|---|
| [BR-ASDLC-001](BR-ASDLC-001.md) | The repository carries no instance data. | Enforced |
| [BR-ASDLC-002](BR-ASDLC-002.md) | A ticket leaves Post Deploy for Done at the bridge's tick, once every box of its `## Post deploy` section is checked; there is no verb. | Enforced |
| [BR-ASDLC-003](BR-ASDLC-003.md) | Post Deploy stays outside the in-progress group; its verification is served through a validation sub-task. | Enforced |
| [BR-ASDLC-004](BR-ASDLC-004.md) | Release angle sub-items and the brief are created only once the candidate's gate is green, never at the release item's creation. | Enforced |
| [BR-ASDLC-005](BR-ASDLC-005.md) | Every PR is independently deployable. | Proposed |
| [BR-ASDLC-006](BR-ASDLC-006.md) | One PR per ticket by default. | Proposed |
| [BR-ASDLC-007](BR-ASDLC-007.md) | A review round is scoped to its PR. | Enforced |
| [BR-ASDLC-008](BR-ASDLC-008.md) | The state of a ticket follows its PRs, by two rules in order. | Enforced |
| [BR-ASDLC-009](BR-ASDLC-009.md) | A ticket the closure returns to In Progress is unassigned; the agent system handles reprise by role, with nobody named. | Enforced |
| [BR-ASDLC-010](BR-ASDLC-010.md) | The public `spl-asdlc` crate depends on no consumer (no `livela-core`): each runtime/tracker writes its own wrapper on its side, depending on `spl-asdlc` by version. The dependency arrow always points toward ASDLC, never outward. | Enforced |
| [BR-ASDLC-011](BR-ASDLC-011.md) | The bridge is the sole writer of item transitions (except on an item under the hands-off label, `docs/spec/07-the-bridge.md`) and writes them regardless of any agent lock; an advisory lock protects an agent's work on the item's content, never its status. | Proposed |
| [BR-ASDLC-012](BR-ASDLC-012.md) | Everything served to an agent carries a full contract; every ticket generated from a release QA finding carries one, rather than any runtime exempting it by marker. | Enforced |
| [BR-ASDLC-013](BR-ASDLC-013.md) | The consolidation agent documents problems, it does not solve them (it may note an already-obvious solution, never hunt for one). | Proposed |
| [BR-ASDLC-014](BR-ASDLC-014.md) | A setting is defined in exactly one place: a key present in both the repository's `spl-asdlc.toml` and the instance overlay is an error, never an override; the overlay carries instance identities only. | Enforced |
| [BR-ASDLC-015](BR-ASDLC-015.md) | An item the bridge serves to an agent carries its protocol: the agent never depends on reading the method's repository to know what to do. | Enforced |
| [BR-ASDLC-016](BR-ASDLC-016.md) | A red gate always produces urgent work served to an agent, never a silence. | Enforced |
| [BR-ASDLC-017](BR-ASDLC-017.md) | A post-deploy validation sub-task is urgent: production is waiting on it. | Enforced |
| [BR-ASDLC-018](BR-ASDLC-018.md) | The consolidation of a round starts only after its angles: its item is blocked by every angle item of the round, and no approval counts while an angle item is open. | Enforced |
| [BR-ASDLC-019](BR-ASDLC-019.md) | A release closure leaves the tracker as it found it: it archives the items its cycle resolved, never an open one. | Enforced |
| [BR-ASDLC-020](BR-ASDLC-020.md) | A release angle's item follows the normal flow and is Done when its angle PR merges; the candidate's consolidation is the overall review, served only once every angle is merged. | Enforced |
| [BR-ASDLC-021](BR-ASDLC-021.md) | Every step of the flow can be done by a human in GitHub; the bridge automates the steps and is never the only actor able to perform them. | Enforced |
| [BR-ASDLC-022](BR-ASDLC-022.md) | A release QA finding is blocking or not: a blocking one becomes an urgent fix ticket assigned to the release fixer and blocks the GO until fixed in a later candidate; a non-blocking one becomes a ticket logged for later. | Proposed |
