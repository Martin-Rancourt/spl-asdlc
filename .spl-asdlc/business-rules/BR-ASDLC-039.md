# BR-ASDLC-039 — Before the rulesets, `doctor` names every branch and open PR that would block the method once they are on, with the gesture to clear it.

Status: Enforced

## Rule

`doctor` has a check `branches and PRs`, read before the ruleset checks. It reads the branches under `release.branch_prefix` and the repository's open PRs, and it flags:

- a branch under the prefix whose name is not a release branch of the method (`<prefix><YYYY-MM-DD-HH-MM>-utc`): the cut refuses while any branch is under the prefix;
- a branch under the prefix named as a release branch whose every commit is in the default branch: a finished cycle's leftover, which the cut takes as a cycle in flight;
- an open PR, draft or not, whose title the PR title check refuses: not `<type>(<scope>)?!?: <summary>` with a type of `release.changelog_sections`.

A release branch with commits beyond the default branch is a cycle in flight and is never flagged. Anything flagged makes the check `invalid`, with each branch and PR and its reason. Its fix says to delete the branches and retitle the PRs before installing the rulesets: once the release prefix's ruleset is on, deleting such a branch needs a bypass. Nothing flagged is `ok`, with what was read. A list that cannot be read is `missing`, never `ok`. `doctor` writes nothing.

## Evidence

ASDLC-SZKK. On 2026-10-25 the first cut of a project set up after its hand releases was refused (« A release branch is already in flight: release/0.1.18 »), and removing that branch needed the `release/**` ruleset disabled for a moment. On 2026-10-26 the owner (voice session) widened the fix to every branch and open PR that would cause a problem once the method is on.

## Test

`doctor::tests::leftover_release_branches_and_refused_pr_titles_are_flagged_before_the_rulesets`, `doctor::tests::the_title_check_is_read_as_the_title_workflow_reads_it`
