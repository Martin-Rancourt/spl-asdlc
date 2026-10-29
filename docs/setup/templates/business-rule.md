# K7QF — Every exported file carries its signature.

Status: Proposed

## Rule

An export the product writes (CSV, PDF or archive) is signed with the product's key before it leaves the server; an export whose signature fails is never sent. The rule holds for every export, old or new, not only the one the ticket that introduced it touched.

## Evidence

Why the rule exists: the incident or the decision that created it, with its date. Here: an unsigned export was altered between the server and the customer, who could not tell.

## Test

The test(s) that enforce it, as CI reports them, and the job that runs them: `tests/export.rs::every_export_is_signed`, in the `test` job. Until that test runs in CI the status stays `Proposed`; set `Status: Enforced` in the PR that makes it run.
