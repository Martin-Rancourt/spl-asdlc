# Review a release candidate from one angle

**Input**: an angle sub-item of a release issue, the brief, the evidence pack, the candidate tag. **Output**: a PR from `qa/<candidate>/<angle>` to the release branch with the fixes of your blocking findings and your report file.

1. Cut your branch: `git switch -c qa/<candidate>/<angle> <candidate tag>` on the release branch's history.
2. Read the brief (embarked tickets and their acceptance criteria, business rules touched, evidence pack) and the tag annotation (release class). Then review the **whole project** from your angle, not the diff: the diff orients you to what changed; your scope is the current state. Run what you need to run: suites, the application, scripts. Record in `checked` what you actually examined.
3. For each finding, decide: `blocker` (must not ship), `should-fix` (real, later), `nit`. Fix every `blocker` you can in your branch, with the test that would have caught it, in the same branch. A `blocker` you cannot fix stays in the report unfixed and becomes a blocker ticket.
4. Write the report `.spl-asdlc/qa/<candidate>/<angle>.json` (schema in `docs/spec/06-qa-report.md`). It is mandatory even with no finding: `checked` says what you looked at, `findings` is empty, `verdict` is `go`.
5. Commit, push, open the PR to the release branch. Body: your sub-item's id, the verdict, the count of blockers fixed and unfixed. The coordinator reviews your PR; answer its threads; the bridge merges it.

You never write in the tracker: the bridge creates the tickets from your report.
