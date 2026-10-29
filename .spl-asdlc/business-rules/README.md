# SPL-ASDLC business rules

One file per rule, `<ID>.md`, in the format of `docs/reference/02-business-rules.md`. Ids are `BR-ASDLC-NNN` and are never reused. A rule is `Enforced` only when the test its file names runs in CI; until then it is `Proposed`.

This page is an index for people, kept by hand; the method never reads it, and a test checks that it lists every rule file and nothing else.

| ID | Rule | Status |
|---|---|---|
| [BR-ASDLC-001](BR-ASDLC-001.md) | The repository carries no instance data. | Enforced |
| [BR-ASDLC-002](BR-ASDLC-002.md) | A ticket leaves Post Deploy for Done at the bridge's tick, once every box of its `## Post deploy` section is checked; there is no verb. | Enforced |
| [BR-ASDLC-003](BR-ASDLC-003.md) | A ticket in Post Deploy is its own validation: no sub-task; an agent checks what it verifies in production and hands the same ticket to the owner for the rest. | Enforced |
| [BR-ASDLC-004](BR-ASDLC-004.md) | Release angle sub-items and the brief are created only once the candidate's gate is green, never at the release item's creation. | Enforced |
| [BR-ASDLC-005](BR-ASDLC-005.md) | Every PR is independently deployable. | Proposed |
| [BR-ASDLC-006](BR-ASDLC-006.md) | One PR per ticket by default. | Proposed |
| [BR-ASDLC-007](BR-ASDLC-007.md) | A review round is scoped to its PR. | Enforced |
| [BR-ASDLC-008](BR-ASDLC-008.md) | The state of a ticket follows its PRs, by two rules in order. | Enforced |
| [BR-ASDLC-009](BR-ASDLC-009.md) | A ticket the closure returns to In Progress is unassigned; the agent system handles reprise by role, with nobody named. | Enforced |
| [BR-ASDLC-010](BR-ASDLC-010.md) | The public `spl-asdlc` crate depends on no consumer (no `livela-core`): each runtime/tracker writes its own wrapper on its side, depending on `spl-asdlc` by version. The dependency arrow always points toward ASDLC, never outward. | Enforced |
| [BR-ASDLC-011](BR-ASDLC-011.md) | The bridge is the sole automated writer of item transitions and writes them regardless of any agent lock; an advisory lock protects an agent's work on the item's content, never its status. | Proposed |
| [BR-ASDLC-012](BR-ASDLC-012.md) | Everything served to an agent carries a full contract; every ticket generated from a release QA finding carries one, rather than any runtime exempting it by marker. | Enforced |
| [BR-ASDLC-013](BR-ASDLC-013.md) | The consolidation agent documents problems, it does not solve them (it may note an already-obvious solution, never hunt for one). | Proposed |
| [BR-ASDLC-014](BR-ASDLC-014.md) | A setting is defined in exactly one place: a key present in both the repository's `.spl-asdlc/config.toml` and the instance overlay is an error, never an override; the overlay carries instance identities only. | Enforced |
| [BR-ASDLC-015](BR-ASDLC-015.md) | An item the bridge serves to an agent carries its protocol: the agent never depends on reading the method's repository to know what to do. | Enforced |
| [BR-ASDLC-016](BR-ASDLC-016.md) | A red gate always produces urgent work served to an agent, never a silence. | Enforced |
| [BR-ASDLC-017](BR-ASDLC-017.md) | A validation sub-task an earlier version created is canceled once, with a comment that sends its reader to the ticket. | Enforced |
| [BR-ASDLC-018](BR-ASDLC-018.md) | The consolidation of a round starts only after its angles: its item is blocked by every angle item of the round, and no approval counts while an angle item is open. | Enforced |
| [BR-ASDLC-019](BR-ASDLC-019.md) | A release closure leaves the tracker as it found it: it archives the items its cycle resolved, never an open one. | Enforced |
| [BR-ASDLC-020](BR-ASDLC-020.md) | A release angle's item follows the normal flow and is Done when its angle PR merges; the candidate's consolidation is the overall review, served only once every angle is merged. | Enforced |
| [BR-ASDLC-021](BR-ASDLC-021.md) | Every step of the flow can be done by a human in GitHub; the bridge automates the steps and is never the only actor able to perform them. | Enforced |
| [BR-ASDLC-022](BR-ASDLC-022.md) | A release QA finding is blocking or not: a blocking one becomes an urgent fix ticket assigned to the release fixer and blocks the GO until fixed in a later candidate; a non-blocking one becomes a ticket logged for later. | Proposed |
| [BR-ASDLC-023](BR-ASDLC-023.md) | spl-asdlc is the one project outside its own method: it releases with release-please and its tickets are Done at merge, so that a bug of the flow never blocks the release that fixes it. | Enforced |
| [BR-ASDLC-024](BR-ASDLC-024.md) | The project owns its CI content; the method checks always run on every PR and cannot be turned off. | Enforced |
| [BR-ASDLC-025](BR-ASDLC-025.md) | Agents' process instructions come from ASDLC, whatever the project; a project outside ASDLC keeps its own in its own repository, never in ASDLC. | Proposed |
| [BR-ASDLC-026](BR-ASDLC-026.md) | The process page reaches a project only through an ASDLC update, pinned on the same version as its workflows; no agent runtime fetches or injects process rules of its own. | Enforced |
| [BR-ASDLC-027](BR-ASDLC-027.md) | ASDLC has one procedure, not flows: configurations differ only by which steps are present, a step absent from the configuration is skipped, and no configuration changes how a present step works. | Enforced |
| [BR-ASDLC-028](BR-ASDLC-028.md) | The delivered process page is never edited in the project: the project only adds its own instructions around it, the page wins on conflict, and a mandatory method check refuses any PR that changes it. | Enforced |
| [BR-ASDLC-029](BR-ASDLC-029.md) | The method's review angles are an optional library; the method checks are not. | Enforced |
| [BR-ASDLC-030](BR-ASDLC-030.md) | An angle may deliver LLM instructions, a reusable workflow, or both; a project runs an angle's workflow only by calling it from its own CI, pinned on the same version, and no switch or flag decides it. | Enforced |
| [BR-ASDLC-031](BR-ASDLC-031.md) | A project adds to a method angle, never replaces it: its notes complement the method's text, and a different angle is the project's own, under another key. | Enforced |
| [BR-ASDLC-032](BR-ASDLC-032.md) | The protocol and angle texts an item carries are those of the version of spl-asdlc the project is pinned on, the same as its workflows; texts that cannot be read there create no item, and the bridge never copies other texts instead. | Enforced |
| [BR-ASDLC-033](BR-ASDLC-033.md) | The project owns when an angle runs and who does it; the method's angle carries only its content. | Enforced |
| [BR-ASDLC-034](BR-ASDLC-034.md) | Adding or retiring a method angle is a normal change of the method; a retired angle stays available to projects pinned on an older version and is reported at their update. | Enforced |
| [BR-ASDLC-035](BR-ASDLC-035.md) | Everything the bridge writes is in English, with no message catalogue. | Proposed |
| [BR-ASDLC-036](BR-ASDLC-036.md) | A PR branch is named `<type>/<ticket-id>-<description>`, in lowercase, and the ticket id in it is enough to link the PR to its ticket. | Enforced |
| [BR-ASDLC-037](BR-ASDLC-037.md) | A ticket whose PR is opened always goes through In Review; with no review configured, one pass writes In Review, merges and writes To Deploy. | Enforced |
| [BR-ASDLC-038](BR-ASDLC-038.md) | A release candidate's QA lives in its own milestone; a newer candidate redoes every review and cancels the older one's open QA items; PR reviews stay in GitHub, mirrored read-only. | Enforced |
| [BR-ASDLC-039](BR-ASDLC-039.md) | Before the rulesets, `doctor` names every branch and open PR that would block the method once they are on, with the gesture to clear it. | Enforced |
