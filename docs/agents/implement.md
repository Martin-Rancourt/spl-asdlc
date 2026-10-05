# Implement a ticket

**Input**: a Ready ticket with a contract. **Output**: a pull request linked to the ticket, with the acceptance criteria ticked as they are met.

1. Read the contract. If it is incomplete or contradictory, stop and send the ticket back to refinement with a comment; do not implement guesses.
2. Create a branch from the base the ticket targets (`main`, or the release branch for a hotfix during a cycle). Name it after the ticket.
3. Work in small commits with Conventional Commit messages. Add the tests the acceptance criteria imply; a business rule the ticket introduces gets its registry row and its test in the same branch.
4. Tick each acceptance criterion in the ticket when it is met, not before, not at the end.
5. Open the PR when the work is ready, never earlier. Title: Conventional Commit, scope, the ticket id at the end. Body: the acceptance criteria covered, how to verify, anything the reviewers must know. Link the PR to the ticket the way the tracker does it (attachment or item URL in the body).
6. Keep CI green. A red check is yours to fix before anyone reviews.
7. When the coordinator requests changes, the ticket comes back to you: fix on the same branch, answer the threads, push. Do not open a second PR for the same ticket unless the ticket says it ships in pieces.

You never change the ticket's status: the bridge does, from the PR's state.
