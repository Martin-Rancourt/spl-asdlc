# BR-ASDLC-018 — The consolidation of a round starts only after its angles: its item is blocked by every angle item of the round, and no approval counts while an angle item is open.

Status: Enforced

## Rule

The bridge creates the consolidation item with the round, so the whole round is visible from the start, and lays a `blocks` relation from every angle sub-item of the round to it, right after the items and before the round comment and In Review; a relation already there is not written again, an open round missing one gets it at the next tick, and a relation that cannot be read or written is reported and retried, never skipped. The tracker stops counting a relation as blocking once the angle item is resolved (Livela: the `closed` group, `done` or `cancel`, `docs/livela-contract.md` §1; Linear: the blocking issue completed or canceled). In case a dispatcher ignores the relation, a coordinator approval does not count while an angle sub-item of the PR's round (the round at its head, or every round of the PR when none is at its head) is open: the PR is not merged and is told once that it waits for the angle reviews (`docs/spec/04-pr-review.md` §3, §6). The release QA round lays the same relations under its own rule, BR-ASDLC-020 (`docs/open-points.md` #15).

## Evidence

The owner's decision of 2026-10-09 (option A), after the first real PR review: a dispatcher served the consolidation item at once, and the coordinator approved the PR before the angle reviewer had submitted anything.

## Test

`tick::tests::the_consolidation_item_is_blocked_by_every_angle_of_its_round_once`, `tick::tests::a_round_missing_its_relations_gets_them_at_the_next_tick`, `tick::tests::a_relation_that_cannot_be_written_holds_the_round_and_is_retried`, `tick::tests::an_approval_while_an_angle_is_open_does_not_merge_and_says_why_once`, `tick::tests::a_dry_run_says_the_relations_it_would_write`
