# Agent protocols

Runtime-agnostic instructions an agent loads before doing one of the method's jobs. They assume nothing about how the agent is run, which model it is, or which tracker serves it: a tracker item says what to do and links to the PR or the candidate; the protocol says how. The bridge embeds the protocol an item needs in the item itself when it creates it (BR-ASDLC-015): an angle sub-item carries `review-pr.md` or `review-release.md` and the angle's file (`angles/<angle>.md`, or a line saying the method has none for a project's own angle), a consolidation item carries `consolidate.md`, and the items of a release candidate also carry the report schema and rules of `docs/reference/06-qa-report.md`. The texts are those of the version of spl-asdlc the project is pinned on, the ref of its workflow callers (BR-ASDLC-032): the bridge reads them in the method's repository at that ref when it creates the item, and the item keeps them while open; the project's own angle notes stay in the project's repository and the item links them. Each protocol ends with what the agent must leave behind (a PR, a report file, review threads) so that the bridge can continue without reading free text.

| File | Job |
|---|---|
| `process.md` | The one procedure: which steps the project's configuration enables, the ticket's statuses and who moves them, refine a ticket, implement it (branch, PR, merge), the release cycle, what an agent never does by hand |
| `review-pr.md` | Review a diff from one angle and post findings on the PR |
| `consolidate.md` | Read every angle's findings, decide, fix, approve (PR review); review each angle PR as it comes, then the overall review once every angle PR is merged (release QA) |
| `review-release.md` | Review a whole candidate from one angle, on an angle branch, with a report |
| `angles/*.md` | What each angle looks for (security, design, product, maintainability); `angles/README.md` lists the library and the angles it retired |

`process.md` reaches a project another way: it is copied into the project's repository, at `.spl-asdlc/process.md`, at the version its workflows pin, and the project's agent file (`AGENTS.md`) points to it (`docs/setup/repository.md`, "The process page"). The project adds its own instructions around it, never inside it, and the method check of `asdlc-pr-guards.yml` refuses a PR whose copy differs from the pinned version's (BR-ASDLC-025 to BR-ASDLC-028).
