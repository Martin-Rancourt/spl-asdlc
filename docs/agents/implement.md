# Implement a ticket

**Input**: a Ready ticket with a contract; you take work from Ready only. **Output**: a pull request linked to the ticket, with the acceptance criteria ticked as they are met.

1. Read the contract. If it is incomplete or contradictory, stop and send the ticket back to refinement with a comment; do not implement guesses. Otherwise move the ticket to In Progress as you start (with Livela, "implement the next ticket" does it when it serves you the ticket). The bridge never does it for you: a branch or a draft PR leaves the ticket where it is (a draft PR is out of the flow, ignored until it is marked ready), and the PR opened puts it In Review.
2. Create a branch from the base the ticket targets (`main`, or the release branch for a hotfix during a cycle, such as a gate-fix ticket the bridge opened on a red gate: its contract names the release branch, and its PR's base is that branch). Name it after the ticket. Never stack it on another PR's branch: with a squash merge, a stacked PR carries the other PR's diff into the base under its own merge, and the bridge never sees that other PR, nor its ticket, merged.
3. Work in small commits with Conventional Commit messages. Add the tests the acceptance criteria imply; a business rule the ticket introduces gets its file in `.spl-asdlc/business-rules/` and its test in the same branch.
   - A new configuration key goes to the code, the configuration spec and the template, never yet into the project's own `.spl-asdlc/config.toml`: the bridge on the host runs a published release, which refuses a key it does not know and then stops serving the repository. The key enters that file in a later PR, once a release that knows it is installed; until then its default holds.
4. Tick each acceptance criterion in the ticket when it is met, not before, not at the end.
5. Open the PR when the work is ready, never earlier. Title: Conventional Commit, scope, the ticket id at the end. Body: the acceptance criteria covered, how to verify, anything the reviewers must know. Link the PR to the ticket the way the tracker does it (attachment or item URL in the body).
6. Keep CI green. A red check is yours to fix before anyone reviews.
7. When the coordinator requests changes, the ticket comes back to you: fix on the same branch, answer the threads, push.
8. One PR per ticket is the default, with as many commits as needed (BR-ASDLC-006). Open a second PR for the same ticket only for a real reason, and record that reason in a comment on the ticket before you open it. A later PR also starts from the base (step 2), once the earlier one is merged. Every PR must be deployable on its own (BR-ASDLC-005): once merged it may ship at any time, even if the ticket is not done.
9. A merged PR that leaves boxes open sends the ticket back to you (In Progress): carry on, tick what each PR delivers, and open the next PR, until every box is ticked.

Moving the ticket to In Progress is yours; the later moves are the bridge's, from the PR's state: In Review when the PR is opened, To Deploy at the merge once every box is ticked (Done when there is nothing to deploy).
