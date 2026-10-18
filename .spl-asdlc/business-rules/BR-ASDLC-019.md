# BR-ASDLC-019 — A release closure leaves the tracker as it found it: it archives the items its cycle resolved, never an open one.

Status: Enforced

## Rule

Once the release issue is closed, the bridge archives the resolved children of the release issue (angle sub-items, consolidation item, any other child), the nits ticket once resolved, the resolved PR review round items of the embarked tickets (recognised by their bridge markers) and the embarked tickets in Done, then the tickets with nothing to deploy (the no-deploy label) that went Done at their merge and whose latest merged PR the release's range holds, with their resolved round items, the release issue last. A ticket in Post Deploy or In Progress, a finding in Triage, an open nits ticket, an item with an open sub-item or in a state the status table does not place is never archived. An item already archived is skipped; a failed archive is reported and retried at the next tick and never blocks nor undoes the closure. Archiving is a tracker capability: Linear archives (`issueArchive`: searchable, restorable); Livela has no open-item limit and archives nothing (`docs/reference/livela-contract.md`).

## Evidence

The owner's decision of 2026-10-09: the tracker workspace reached the open-item limit of Linear's free plan (250) during step 4a, and the bridge was blocked without saying so. The sandbox run of 2026-10-15: a ticket Done at its merge through the no-deploy label was never archived.

## Test

`method::release::closure::tests::the_closure_archives_resolved_items_and_never_an_open_one`, `tick::release_tests::the_closure_archives_what_the_cycle_resolved_once_and_never_an_open_item`, `tick::release_tests::an_open_nits_ticket_stays_and_an_open_child_keeps_the_release_item`, `tick::release_tests::a_failed_archive_never_undoes_the_closure_and_is_retried`, `tick::release_tests::a_tracker_without_an_open_item_limit_archives_nothing`, `tick::release_tests::a_release_item_closed_by_hand_is_not_archived_from`, `tick::release_tests::a_dry_run_of_the_closure_prints_the_archives`, `tick::release_tests::labelled_round_items_resolved_are_archived_once_with_the_cycle`, `tick::release_tests::a_ticket_with_nothing_to_deploy_is_never_embarked`, `tick::release_tests::a_ticket_with_nothing_to_deploy_whose_latest_pr_is_after_the_range_is_not_archived`, `tracker::linear::tests::an_item_is_archived_never_trashed`, `tracker::linear::tests::an_archive_whose_answer_is_lost_is_read_back`, `tracker::livela::tests::livela_has_no_open_item_limit_and_archives_nothing`, `dry::tests::a_dry_run_reads_for_real_and_writes_nothing`
