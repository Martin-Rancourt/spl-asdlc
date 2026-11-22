# 4HQX — A held PR is never recorded as passing.

Status: Enforced

## Rule

The PR status the GitHub sync records on an open ticket whose contract the bridge holds is the held state (`open:held`, followed by the missing parts one per line), with the missing parts of the same `contract::Verdict` the gate, the hold comment and the `spl-asdlc / contract` check read (C9VD), never a computation of its own. It applies to an open, ready PR whose checks do not fail: a draft stays a draft, failing checks stay failing (the bridge waits on CI before it reads the contract). Once the ticket is completed, the next pass records the PR's checks again, with no new commit.

## Evidence

The owner's request of 2026-11-02, after a held PR was shown on its ticket as « open, checks passing » (the check rollup leaves the contract check out, M6TQ). Split from the tracker's display of a held PR.

## Test

`sync::tests::a_pr_held_by_its_tickets_contract_is_recorded_held_until_the_ticket_is_completed`, `sync::tests::only_an_open_ready_pr_whose_checks_do_not_fail_is_held`, `tracker::tests::a_held_pr_status_round_trips_through_its_code_and_its_sentence`, `tracker::livela::tests::the_github_sync_links_and_records_on_livela_signed_by_the_bridge` (unit test job).
