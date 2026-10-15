# 06 — The angle report

Every angle of a release QA commits one report in its branch, in `.spl-asdlc/qa/<candidate>/<angle>.json` by default (the directory is `release.record`, `08-configuration.md`). The report is mandatory: an angle that found nothing still commits it with an empty findings list and a `go` verdict. The angle's PR carries this file and nothing else: an angle documents, it fixes nothing (BR-ASDLC-022). The bridge reads the merged reports to create tickets and to compute the GO; the concatenation of the reports is the record of the cycle.

## Schema

```json
{
  "schema": "spl-asdlc/qa-report/2",
  "candidate": "<rc tag>",
  "angle": "<angle key>",
  "item": "<the key of the angle sub-item this report answers, e.g. ABC-123>",
  "verdict": "go | hold",
  "checked": ["<what was actually examined: paths, suites run, scenarios exercised>"],
  "findings": [
    {
      "id": "<angle>-<n>",
      "blocking": true,
      "summary": "<what was observed, one line>",
      "anchor": "<where: path:line, route, screen or scenario>",
      "evidence": "<in what context: how it was met, what it looks like, output or a screenshot path in the evidence pack>",
      "testable": true,
      "proposed_test": "<test name or description>"
    }
  ]
}
```

`item` names the angle sub-item, never the reviewer: a report is committed to the repository, which carries no instance data (BR-ASDLC-001), and the tracker already knows who holds the sub-item. The bridge reads the reviewer from the tracker when it needs one, never from the report. A report merged before `item` existed carries a `reviewer` field instead; the bridge still accepts it and ignores the field.

A report merged before findings were blocking or not (`spl-asdlc/qa-report/1`: a `severity` of `blocker`, `should-fix` or `nit`, a `fixed_in_branch`, a `go-with-note` verdict) is still read from the merged history: a `blocker` as blocking, a `should-fix` or a `nit` as non-blocking, `go-with-note` as `go`, and a finding it marked `fixed_in_branch` is not filed. An open angle PR must carry the current schema.

## Rules

1. `checked` is never empty: "found nothing" is not "did nothing". A report with an empty `checked` is invalid and the bridge refuses the angle PR.
2. Each finding is `blocking` (`true`: the release must not ship with it) or not (`false`: real, to fix later). The verdict follows: `hold` when at least one finding is blocking, `go` otherwise. A finding carries no severity and no fix: the angle fixes nothing.
3. A blocking finding becomes, when the report is merged, an **urgent fix ticket**: in Ready, assigned to `release.fix_assignee` (unassigned, taken by role, when the key is unset or unknown to the tracker), labelled with the release label, carrying the blocker marker, and `blocks` the release issue, so that while it is open there is no GO; its fix merged to the release branch makes the next candidate. The bridge writes nothing back into the report: the finding marker on the ticket (candidate, angle, finding id) carries the mapping.
4. A non-blocking finding becomes an ordinary ticket **logged for later**: in Triage, labelled `qa-finding` and with the release label, normal priority, unassigned, related to the release issue. Every finding's ticket is created by the bridge, once, idempotently (keyed by candidate, angle, finding id, whatever became of the ticket), with a full contract (BR-ASDLC-012): a goal, non-goals, the finding under `## Acceptance criteria` as the report gives it, and `No new rule.` under `## Business rules`. There is no nits ticket.
5. A finding is documented, not solved (BR-ASDLC-013): `summary` says what was observed, `anchor` where, `evidence` in what context (how it was met, what it looks like), as faithfully as possible, since whoever takes its ticket up has those words alone. `evidence` may end with a fix that is already obvious; nobody hunts for one. The bridge copies the three into the ticket's criterion, each collapsed to one line.
6. Every `testable: true` finding names a `proposed_test`; the ticket that fixes it carries that test as an acceptance criterion.
7. A red CI suite is not a finding: it is the gate's job.

## Consolidation

The coordinator reviews each angle PR as it comes, as its reviewer: that review is the consolidation of that angle. In the PR threads it confirms or rejects each finding, exactly once, with a reason for every rejection; it may change whether a finding is blocking, with a reason; it resolves conflicts with other angle branches. The bridge checks, before merging an angle PR, that the report is valid (schema, rules 1 and 2), that the PR changes nothing but the report (an angle PR with any other file is not merged, and is told so once per head on the PR), and that the coordinator approved at the head SHA. The report carries no mark of a rejection: a rejected finding stays in the report and the coordinator's verdict is recorded in the thread, with its reason. The bridge files every finding of a merged report (rules 3 and 4), rejected or not; the coordinator then says the rejection on the finding's ticket, with the reason and the thread, and the ticket is canceled with that reason (Canceled is the owner's to write, `03-vocabulary.md`); a canceled finding is never filed again (the merged report does not change). A finding filed with the wrong weight is requalified (`09-requalification-and-human-gestures.md`). Once every angle PR of the candidate is merged, the candidate's consolidation item serves the **overall review** (`05-release-cycle.md` §5, BR-ASDLC-020): the coordinator reads every merged report together, confirms or rejects each finding filed from them and whether it is blocking, and checks that nothing falls between the angles.

## Prompts

The angle protocols are documentation (`docs/agents/review-release.md`, one base plus one short file per angle: security, design, product, maintainability). A project may add angle notes in `.spl-asdlc/angles/<angle>.md`. The report schema above is what every protocol ends with. The bridge embeds the base protocol, the angle's file and the schema and rules above in each angle sub-item and consolidation item of a candidate, and links the project's notes (BR-ASDLC-015, `05-release-cycle.md` §3).
