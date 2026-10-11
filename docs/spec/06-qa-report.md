# 06 — The angle report

Every angle of a release QA commits one report in its branch, in `.spl-asdlc/qa/<candidate>/<angle>.json`. The report is mandatory: an angle that found nothing still commits it with an empty findings list and a `go` verdict. The bridge reads the merged reports to create tickets and to compute the GO; the concatenation of the reports is the record of the cycle.

## Schema

```json
{
  "schema": "spl-asdlc/qa-report/1",
  "candidate": "<rc tag>",
  "angle": "<angle key>",
  "item": "<the key of the angle sub-item this report answers, e.g. ABC-123>",
  "verdict": "go | go-with-note | hold",
  "checked": ["<what was actually examined: paths, suites run, scenarios exercised>"],
  "findings": [
    {
      "id": "<angle>-<n>",
      "severity": "blocker | should-fix | nit",
      "summary": "<one line>",
      "anchor": "<path:line, route, screen or scenario>",
      "evidence": "<what proves it: output, screenshot path in the evidence pack, reproduction>",
      "fixed_in_branch": true,
      "testable": true,
      "proposed_test": "<test name or description>",
      "ticket": null
    }
  ]
}
```

`item` names the angle sub-item, never the reviewer: a report is committed to the repository, which carries no instance data (BR-ASDLC-001), and the tracker already knows who holds the sub-item. The bridge reads the reviewer from the tracker when it needs one, never from the report. A report merged before `item` existed carries a `reviewer` field instead; the bridge still accepts it and ignores the field.

## Rules

1. `checked` is never empty: "found nothing" is not "did nothing". A report with an empty `checked` is invalid and the bridge refuses the angle PR.
2. A `hold` verdict requires at least one `blocker` that is not `fixed_in_branch`; a `go` verdict requires no `blocker` at all; otherwise `go-with-note`.
3. A `blocker` fixed in the branch is listed with `fixed_in_branch: true` and its test named in `proposed_test`; the fix and the test are commits of the same branch.
4. A `blocker` not fixed in the branch becomes, when the report is merged, a blocker ticket that `blocks` the release issue; the bridge writes its id back into `ticket` in a follow-up commit on the release branch, or records the mapping in the release issue.
5. A `should-fix` becomes a `qa-finding` ticket; a `nit` becomes a box of the cycle's nits ticket, or a ticket of its own, as `release.nits` says (`05-release-cycle.md` §6). All are created by the bridge, once, idempotently (keyed by candidate, angle, finding id), each with a full contract. A `nit` is documented, not solved: `summary` says what was observed, `anchor` where, `evidence` in what context (how it was met, what it looks like). `evidence` may end with a fix that is already obvious; nobody hunts for one. The bridge copies the three into the nit's box, each collapsed to one line.
6. Every `testable: true` finding names a `proposed_test`; the ticket that fixes it carries that test as an acceptance criterion.
7. A red CI suite is not a finding: it is the gate's job.

## Consolidation

The coordinator reviews each angle PR as it comes, as its reviewer: that review is the consolidation of that angle. In the PR threads it confirms or rejects each finding, exactly once, with a reason for every rejection; it may lower or raise a severity with a reason; it resolves conflicts with other angle branches. The bridge checks, before merging an angle PR, that the report is valid (schema, rules 1 and 2) and that the coordinator approved at the head SHA. The report carries no mark of a rejection: a rejected finding stays in the report and the coordinator's verdict is recorded in the thread, with its reason. The bridge files every finding of a merged report that is not `fixed_in_branch` (rules 4 and 5), rejected or not; the coordinator then says the rejection on the finding's ticket, with the reason and the thread, and the ticket is canceled with that reason (Canceled is the owner's to write, `03-vocabulary.md`); a canceled finding is never filed again (the merged report does not change). A rejected `nit` of a shared nits ticket is said on that ticket, with the reason, and its triage drops it. Once every angle PR of the candidate is merged, the candidate's consolidation item serves the **overall review** (`05-release-cycle.md` §5, BR-ASDLC-020): the coordinator reads every merged report together, confirms or rejects the non-blocking findings filed from them, and checks that nothing falls between the angles.

## Prompts

The angle protocols are documentation (`docs/agents/review-release.md`, one base plus one short file per angle: security, design, product, maintainability). A project may add angle notes in `.spl-asdlc/angles/<angle>.md`. The report schema above is what every protocol ends with. The bridge embeds the base protocol, the angle's file and the schema and rules above in each angle sub-item and consolidation item of a candidate, and links the project's notes (BR-ASDLC-015, `05-release-cycle.md` §3).
