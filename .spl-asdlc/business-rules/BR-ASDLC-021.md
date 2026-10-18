# BR-ASDLC-021 — Every step of the flow can be done by a human in GitHub; the bridge automates the steps and is never the only actor able to perform them.

Status: Enforced

## Rule

A person the rulesets let act may merge a delivery PR, an angle PR or the merge-back PR, dispatch the cut or the publication, or tag the release on the latest candidate and publish its GitHub release by hand, and the chain goes on: every workflow and every tick reads the state GitHub is in, never who brought it there, so a step done by hand is taken as done, neither refused, redone nor rewritten. Who may merge or tag is the rulesets' decision, never a workflow's: `release-publish` accepts a release published by the bridge's App or by a human account (a User), refuses any other bot, and says which (`go-published-by-kind`, its summary); the bridge merges nothing a person already merged, tags no release a person already tagged on the candidate, publishes no release a person already published, and records at the closure who published the release. A step that is not a GitHub gesture (the closure, the ticket moves, both tracker writes) says why in spec 09 (`docs/reference/09-requalification-and-human-gestures.md`, `docs/reference/05-release-cycle.md` §9).

## Evidence

The owner's principle, after a release PR the owner could not merge by hand: the publication refused any merger but the bridge's App, and a manual merge would have left the release unpublished with nothing to replay. Reworded on 2026-10-12, when the cycle dropped release-please: the GO a person gives by hand is the release tag and the release, no longer a release PR merged.

## Test

`tests/workflows/release_publish.rs`: `a_release_published_by_a_human_is_the_go_and_says_so`, `a_release_published_by_another_bot_or_nobody_readable_is_no_go`, `a_merge_back_a_human_did_in_github_is_taken_as_done`; `tick::release_tests::a_release_published_by_a_human_is_picked_up_without_a_rewrite_and_recorded_at_closure`, `tick::release_tests::a_release_tag_made_by_hand_on_the_candidate_is_taken_as_done`, `method::release::prs::tests::the_go_publisher_line_says_the_bridge_a_human_or_another_app`, `tick::release_tests::release_retract_retracts_by_hand_once_and_refuses_what_is_not_a_release`
