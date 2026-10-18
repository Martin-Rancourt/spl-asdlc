# The method — specification

> The method in ten minutes is `docs/method.md`. These files are the reference behind it: the same rules with every edge case, for whoever implements or debugs the bridge.

One file per topic. Each file states invariants (numbered), the vocabulary it introduces, and what is configurable per project. Nothing here names a runtime, a tracker or a product: those are adapters and configuration. Every layer above the fixed core is optional per project: the PR review and the release QA angles; the core is the contract, the registry, CI, the bridge, the release cycle, and CI that only publishes facts (`00-overview.md`).

| File | Topic |
|---|---|
| `00-overview.md` | The ten invariants, the actors (implementer, reviewer angles, coordinator, the bridge, the human), the three moments of QA (ticket, diff, project) |
| `01-ticket-contract.md` | Goal, non-goal, acceptance criteria, business rules; the refinement gate; the release-note snippet |
| `02-business-rules.md` | Global, permanent rules; the registry, one file per rule; one rule = one deterministic test; the guard at merge and at closure |
| `03-vocabulary.md` | Canonical statuses (Triage, Backlog, Ready, In Progress, In Review, To Deploy, Post Deploy, Done), severities, verdicts, item kinds |
| `04-pr-review.md` | CI first; angle sub-items; findings as PR review comments; the coordinator fixes on the branch and approves; the bridge merges; "To Deploy" |
| `05-release-cycle.md` | Cut by dispatch; release issue at the next tick; angle branches with a mandatory report; RC passes; computed GO; publish; merge-back; closure; release note |
| `06-qa-report.md` | The angle report schema, findings, severities, consolidation rules |
| `07-the-bridge.md` | One deterministic process; polling; what it reads on GitHub; what it writes to the tracker; what it merges; idempotence; identity |
| `08-configuration.md` | `.spl-asdlc/config.toml`: product, release shape, tracker, angles, `max_rc_passes`, GO policy, merge policy, channels, cadence |
| `09-requalification-and-human-gestures.md` | Cut, requalify a finding, human GO policy, resume, take a PR out of the flow, move a ticket; the audit of every step a person can do by hand in GitHub |
| `10-run-journal.md` | The journal of a release cycle: its template, the gestures outside the method, the defects of the chain |
