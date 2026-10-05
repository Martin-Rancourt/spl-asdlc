# 06 — The angle report

Every angle of a release QA commits one report in its branch, in `.spl-asdlc/qa/<candidate>/<angle>.json`. The report is mandatory: an angle that found nothing still commits it with an empty findings list and a `go` verdict. The bridge reads the merged reports to create tickets and to compute the GO; the concatenation of the reports is the record of the cycle.

## Schema

```json
{
  "schema": "spl-asdlc/qa-report/1",
  "candidate": "<rc tag>",
  "angle": "<angle key>",
  "reviewer": "<participant id or name as the tracker knows it>",
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

## Rules

1. `checked` is never empty: "found nothing" is not "did nothing". A report with an empty `checked` is invalid and the bridge refuses the angle PR.
2. A `hold` verdict requires at least one `blocker` that is not `fixed_in_branch`; a `go` verdict requires no `blocker` at all; otherwise `go-with-note`.
3. A `blocker` fixed in the branch is listed with `fixed_in_branch: true` and its test named in `proposed_test`; the fix and the test are commits of the same branch.
4. A `blocker` not fixed in the branch becomes, when the report is merged, a blocker ticket that `blocks` the release issue; the bridge writes its id back into `ticket` in a follow-up commit on the release branch, or records the mapping in the release issue.
5. A `should-fix` becomes a `qa-finding` ticket; `nit`s go to the cycle's nits ticket. Both are created by the bridge, once, idempotently (keyed by candidate, angle, finding id).
6. Every `testable: true` finding names a `proposed_test`; the ticket that fixes it carries that test as an acceptance criterion.
7. A red CI suite is not a finding: it is the gate's job.

## Consolidation

The coordinator's review of an angle PR is the consolidation of that angle. In the PR threads it confirms or rejects each finding, exactly once, with a reason for every rejection; it may lower or raise a severity with a reason; it resolves conflicts with other angle branches. The bridge checks, before merging an angle PR, that the report is valid (schema, rules 1 and 2) and that the coordinator approved at the head SHA. A rejected finding stays in the report with the coordinator's verdict recorded in the thread; the bridge does not create a ticket for a rejected finding.

## Prompts

The angle protocols are documentation (`docs/agents/review-release.md`, one base plus one short file per angle: security, design, product, maintainability). A project may add angle notes in `.spl-asdlc/angles/<angle>.md`. The report schema above is what every protocol ends with.
