# BR-ASDLC-036 — A PR branch is named `<type>/<ticket-id>-<description>`, in lowercase, and the ticket id in it is enough to link the PR to its ticket.

Status: Enforced

## Rule

A PR's head branch is `<type>/<ticket-id>-<description>`, all in lowercase: `<type>` is the Conventional Commit type of the PR's title, `<ticket-id>` its ticket's id, `<description>` a few words joined by single hyphens (`fix/abc-123-login-timeout`; process page, step 2). The method's own branches (`release/…`, `chore/merge-back-…`, `qa/…`) and Dependabot's keep their names.

The bridge links a PR to its ticket by the id in its branch, or at the end of its title (`… (ABC-123)`), whatever the tracker, comparing ids without regard to case. It never guesses: only an id with one of the project's prefixes whose item carries that very code names a ticket, and an id no item has names none. The tracker's own link wins, and an item URL in the body still names the ticket under `tracker.link_fallback = "body-url"`. A PR found by its id gets the tracker's link from the bridge, so that its release embarks the ticket.

`asdlc-pr-guards.yml` checks the branch's name and gives the expected one, from the title's type and id. On the 1.4 line it only warns (`branch-name-check: warn`, the default), so that PRs already open stay green; `branch-name-check: refuse` refuses it, the next minor line's default.

## Evidence

The owner's decision of 2026-10-26 (ASDLC-VW9U), after PRs whose branch and title named their ticket stayed « No ticket is linked » (ASDLC-ASB8). Linear and Jira link a PR by the id in its branch or title, case-insensitive; Conventional Branch names a branch `<type>/<description>` in lowercase.

## Test

`tick::link::tests::the_branch_and_the_end_of_the_title_name_the_ticket_without_regard_to_case`, `tick::link::tests::an_unknown_or_foreign_id_names_nothing_and_nothing_is_guessed`, `tick::tests::a_pr_named_by_its_branch_is_linked_and_its_ticket_goes_in_review`, `tracker::livela::tests::a_pr_names_its_livela_ticket_by_branch_title_or_either_address`, `workflows::pr_guards::a_branch_named_after_its_ticket_passes_silently`, `workflows::pr_guards::a_misnamed_branch_warns_with_the_expected_name_and_refuses_when_asked`, `workflows::pr_guards::the_branch_check_warns_by_default_on_this_minor_line_and_rejects_a_bad_value`
