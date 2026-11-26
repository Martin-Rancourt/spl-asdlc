# K7P2 — A ticket stays in To Deploy only until a release contains its merged PR.

Status: Enforced

## Rule

At release closure, every ticket whose latest merged PR is in the released range is judged from its contract and moved to Done, Post Deploy or back to In Progress. The released range is the membership fact; a missing release label cannot leave a shipped ticket in To Deploy. Each later bridge pass also recovers a historical ticket already stranded in To Deploy by judging its latest merged PR against the newest release tag through the same closure gate.

## Evidence

ASDLC-P63X: a shipped ticket without its release label remained in To Deploy because closure enumerated only labelled tickets and children of the release item, even though the released range already identified the ticket.

## Test

`tick::release_tests::a_shipped_ticket_without_its_release_label_is_closed_from_the_range`, `tick::tests::a_ticket_stranded_in_to_deploy_after_its_release_is_recovered`
