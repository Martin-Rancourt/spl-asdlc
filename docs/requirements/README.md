# Requirements for the crate and the workflows

Derived from the Python implementation this product replaces and from the two repositories that ran it. Each requirement states what the system must do, cites where the behaviour came from (file and line in the previous implementation) and the tests to carry over, named after their origin. The specification (`docs/spec/`) is normative; where a requirement and the specification disagree, the specification wins and the requirement is to be corrected.

| File | Area | Content |
|---|---|---|
| `merge.md` | The merge pass, the ticket contract and the business-rule guards | 83 requirements, 360 tests carried, 124 dropped with reasons, 33 open questions |
| `release.md` | The release chain: cut, gate reading, brief, QA coordination, GO, publish merge, closure, release note; the `Tracker` trait surface (REQ-REL-098) and the GitHub operations (REQ-REL-099) | 102 requirements, 626 tests carried, 224 dropped, 20 open questions with the editor's decisions |
| `ci.md` | The reusable workflows (`pr-title`, `pr-guards`, `release-cut`, `release-gate`, `release-verify`, `release-record-deployed`, `release-close`, release-please handling, superseded on 2026-10-12 by the cycle's own preparation), the evidence pack contract and the angle report schema | 102 requirements, 420 tests mapped, 18 dropped behaviours, 22 open questions with the editor's decisions |

Some decisions taken after the draft live in the specification only, with no requirement here: the optional PR review (`docs/spec/04-pr-review.md` §11), the hands-off label (`docs/spec/07-the-bridge.md`, "Hands off"), one tick at a time per repository under a lock (`docs/spec/07-the-bridge.md`, invariant 8), and the release preparation by `release prepare` at the cut (`docs/spec/05-release-cycle.md` §1).

Reading order for an implementer: `docs/spec/` first, then `release.md` §Status/tracker writes for the trait, then the rest. Ticket identifiers in examples are placeholders (`ABC-123`); the previous implementation's incidents are referred to as "a past incident".
