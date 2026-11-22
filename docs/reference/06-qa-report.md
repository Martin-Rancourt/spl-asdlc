# 06 — The angle report

Every angle of a release QA commits one report in its branch, in `.spl-asdlc/qa/<candidate>/<angle>.json` by default (the directory is `release.record`, `08-configuration.md`). The report is mandatory: an angle that found nothing still commits it with an empty findings list and a `go` verdict. The angle's PR carries this file and nothing else: an angle documents, it fixes nothing (BR-ASDLC-022). The coordinator judges every finding of the merged reports in its verdict (below); the bridge files tickets from the findings the merged verdict accepts and computes the GO from it. The reports and the verdict together are the record of the cycle.

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
3. A finding the merged verdict accepts as blocking becomes an **urgent fix ticket**: in Ready, assigned to `release.fix_assignee` (unassigned, taken by role, when the key is unset or unknown to the tracker), labelled with the release label, carrying the blocker marker, and `blocks` the release issue, so that while it is open there is no GO; its fix merged to the release branch makes the next candidate. Whether a finding is blocking is the verdict's, which may change the report's, with a reason. A rejected finding files nothing. The bridge writes nothing back into the report or the verdict: the finding marker on the ticket (candidate, angle, finding id) carries the mapping.
4. A finding the merged verdict accepts as non-blocking becomes an ordinary ticket **logged for later**: in Triage, labelled `qa-finding` and with the release label, normal priority, unassigned, related to the release issue. Every finding's ticket is created by the bridge, once, idempotently (keyed by candidate, angle, finding id, whatever became of the ticket), with a full contract (BR-ASDLC-012): a goal, non-goals, the finding under `## Acceptance criteria` as the report gives it, and `No new rule.` under `## Business rules`. There is no nits ticket.
5. A finding is documented, not solved (BR-ASDLC-013): `summary` says what was observed, `anchor` where, `evidence` in what context (how it was met, what it looks like), as faithfully as possible, since whoever takes its ticket up has those words alone. `evidence` may end with a fix that is already obvious; nobody hunts for one. The bridge copies the three into the ticket's criterion, each collapsed to one line.
6. Every `testable: true` finding names a `proposed_test`; the ticket that fixes it carries that test as an acceptance criterion.
7. A red CI suite is not a finding: it is the gate's job.

## The verdict

Once every angle report of a candidate is merged, its coordinator commits the **verdict** at `release.record` rendered for the candidate with the reserved key `consolidation` (`.spl-asdlc/qa/<candidate>/consolidation.json` by default; no release angle may take that key), from the branch `qa/<candidate>/consolidation`, in a PR to the release branch that names the candidate's consolidation item and carries the verdict and nothing else (rule V7QD).

```json
{
  "schema": "spl-asdlc/qa-verdict/1",
  "candidate": "<rc tag>",
  "item": "<the key of the consolidation item this verdict answers>",
  "verdict": "go | hold",
  "between_angles": "<what was checked across the reports: a concern no angle owned, two angles that each assumed the other covered it>",
  "findings": [
    { "angle": "<angle key>", "id": "<finding id>", "decision": "accepted", "blocking": true, "reason": "<optional for an accepted finding>" },
    { "angle": "<angle key>", "id": "<finding id>", "decision": "rejected", "reason": "<why: required>" }
  ]
}
```

1. Every finding of every merged report of the candidate is judged exactly once, by its `angle` and `id`; a finding no merged report has is refused. `findings` is empty only when no report found anything.
2. An accepted finding says whether it is `blocking`, which may differ from its report's; a rejected finding always carries its `reason` and is never blocking.
3. `between_angles` is never empty, as a report's `checked`.
4. The verdict is `hold` when it accepts a blocking finding; a `go` with an accepted blocking finding is invalid. A `hold` with none is valid: the coordinator holds the candidate for a reason it gives in `between_angles`.
5. `item` names the consolidation item, never a person or an agent (BR-ASDLC-001): the verdict's commit and the tracker say who.

The bridge merges the verdict PR when the verdict is valid, every angle PR of the candidate is merged, its author is one of `release.coordinators` or one of them approved it at its head, its required checks are green and it is up to date; what else keeps it from the merge is said once per head, on the PR. At its merge it files the accepted findings (rules 3 and 4), each once, and moves the consolidation item to Done. The GO of a candidate with QA angles needs its merged verdict `go` (`05-release-cycle.md` §8), and a merged verdict counts only when one of `release.coordinators` authored it or approved it at its merged head, or the bridge's App merged it: a verdict PR merged by hand by anyone else gives no GO. A finding filed with the wrong weight is still requalified (`09-requalification-and-human-gestures.md`).

## Prompts

The angle protocols are documentation (`docs/agents/review-release.md`, one base plus one short file per angle: security, design, product, maintainability). A project may add angle notes in `.spl-asdlc/angles/<angle>.md`. The report schema above is what every protocol ends with. The bridge embeds the base protocol, the angle's file and the schema and rules above in each angle sub-item and consolidation item of a candidate (the consolidation item also carries the verdict's schema and rules), and links the project's notes (BR-ASDLC-015, `05-release-cycle.md` §3): the protocol and the angle's file at the version of spl-asdlc the project is pinned on (BR-ASDLC-032), the schema and rules as the bridge itself checks them.
