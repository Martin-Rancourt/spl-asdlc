# BR-ASDLC-038 — A release candidate's QA lives in its own milestone; a newer candidate redoes every review and cancels the older one's open QA items; PR reviews stay in GitHub, mirrored read-only.

Status: Enforced

## Rule

On a tracker that has milestones (Livela, once its store creates a milestone under a release item), each release candidate's QA is one milestone under the release item, named after the candidate's tag, holding one item per configured QA angle and the candidate's consolidation item. The bridge creates the milestone the first time the candidate has a QA item to create, finds it again by its marker `spl-asdlc:milestone scope=<tag>`, never sets its status (the tracker derives it), and reads the items in it as the release item's own children. A milestone that cannot be read or created creates no QA item; the next tick retries. A tracker without milestones keeps the items directly under the release item.

Every new candidate redoes every review: it gets its own full set of QA items (in its own milestone where there are milestones). In the pass that first sees a newer candidate tagged, every QA item of an older candidate still open (its angle items, its consolidation item) goes Canceled; one already finished stays as it is.

A PR review happens in the GitHub pull request (comments and approvals), by people or agents: the bridge creates no review item in the tracker for it. The GitHub sync mirrors each PR's reviews on the open tickets linked to it, read-only, on a tracker that keeps them: each reviewer's latest verdict (an approval or a request for changes, dismissed or not, stands over a later comment; a pending review counts for nothing), written only when it changed, never from a review list GitHub cut.

## Evidence

The owner's decision of 2026-10-26 (voice brainstorm, ASDLC-JPCU, from AGFG-BCSD): Livela is the owner's own tool, so a release's QA is laid out with its own types, and the reviews stay where people already review.

## Test

`tick::release_tests::on_a_tracker_with_milestones_each_candidate_gets_its_own_qa_milestone`, `tick::release_tests::a_milestone_that_cannot_be_created_creates_no_qa_item_and_is_retried`, `tick::release_tests::a_newer_candidate_tagged_cancels_every_open_qa_item_of_the_older_once`, `tracker::livela::tests::a_store_without_milestones_says_so_and_creates_none`, `tracker::livela::tests::a_milestone_is_created_under_its_parent_signed_and_reads_with_no_status_of_the_method`, `tracker::livela::tests::pr_reviews_are_mirrored_on_a_linked_item_once`, `tracker::tests::a_prs_reviews_are_each_reviewers_latest_verdict_and_round_trip`, `sync::tests::a_prs_github_reviews_are_mirrored_on_its_ticket_read_only_and_once`
