# Agent protocols

Runtime-agnostic instructions an agent loads before doing one of the method's jobs. They assume nothing about how the agent is run, which model it is, or which tracker serves it: a tracker item says what to do and links to the PR or the candidate; the protocol says how. Each protocol ends with what the agent must leave behind (a PR, a report file, review threads) so that the bridge can continue without reading free text.

| File | Job |
|---|---|
| `refine-ticket.md` | Turn a draft ticket into a contract |
| `implement.md` | Take a ticket, work on a branch, open the PR |
| `review-pr.md` | Review a diff from one angle and post findings on the PR |
| `consolidate.md` | Read every angle's findings, decide, fix, approve (PR review and release QA) |
| `review-release.md` | Review a whole candidate from one angle, on an angle branch, with a report |
| `angles/*.md` | What each angle looks for (security, design, product, maintainability) |
