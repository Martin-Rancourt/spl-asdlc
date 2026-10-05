# Requirements for the crate and the workflows

Derived from the Python implementation this product replaces and from the two repositories that ran it. Each requirement states what the system must do, cites where the behaviour came from (file and line in the previous implementation) and the tests to carry over, named after their origin. The specification (`docs/spec/`) is normative; where a requirement and the specification disagree, the specification wins and the requirement is to be corrected.

| File | Area | Content |
|---|---|---|
| `merge.md` | The merge pass, the ticket contract and the business-rule guards | 83 requirements, 360 tests carried, 124 dropped with reasons, 33 open questions |
| `release.md` | The release chain: cut, gate reading, brief, QA coordination, GO, publish merge, closure, release note; the `Tracker` trait surface (REQ-REL-098) and the GitHub operations (REQ-REL-099) | 102 requirements, 626 tests carried, 224 dropped, 20 open questions with the editor's decisions |
| `ci.md` | The reusable workflows, the evidence pack and the angle report schema | see the file |

Reading order for an implementer: `docs/spec/` first, then `release.md` §Status/tracker writes for the trait, then the rest. Ticket identifiers in examples are placeholders (`ABC-123`); the previous implementation's incidents are referred to as "a past incident".
