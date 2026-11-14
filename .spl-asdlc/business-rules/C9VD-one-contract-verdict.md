# C9VD — One contract verdict: the contract check, the hold comment, the merge decision and `ticket check` always say the same thing.

Status: Enforced

## Rule

The contract check on GitHub (`spl-asdlc / contract`), the bridge's hold comment on the PR and its decision request or comment on the ticket, its decision to hold or go on with the PR, and `spl-asdlc ticket check` all read one computation of the ticket's contract, `contract::Verdict`: complete (the check passes, the PR goes on, `ticket check` exits 0), incomplete with the list of what is missing (the check fails and lists it, the comment lists it, the PR is held, `ticket check` lists it and exits 1), or no ticket (no check, no hold for the contract, `ticket check` exits 2). None of them computes the contract on its own.

## Evidence

The owner's decisions of 2026-11-02 and 2026-11-13, ASDLC-57KZ: a PR held for its ticket's contract showed every check green on GitHub; the check that says so must match spl-asdlc's own verdict exactly.

## Test

`tick::tests::the_check_the_comment_the_merge_and_ticket_check_never_disagree`, `tick::tests::a_held_contract_turns_green_at_the_next_pass_once_the_ticket_is_completed` (unit test job).
