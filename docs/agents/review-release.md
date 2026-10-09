# Review a release candidate from one angle

**Input**: an angle sub-item of a release issue, the brief, the evidence pack, the candidate tag. **Output**: a PR from `qa/<candidate>/<angle>` to the release branch with the fixes of your blocking findings and your report file.

1. Cut your branch: `git switch -c qa/<candidate>/<angle> <candidate tag>` on the release branch's history.
2. Read the brief (embarked tickets and their acceptance criteria, business rules touched, evidence pack) and the tag annotation (release class). Then review the **whole project** from your angle, not the diff: the diff orients you to what changed; your scope is the current state. Run what you need to run: suites, the application, scripts. Record in `checked` what you actually examined.
3. For each finding, decide: `blocker` (must not ship), `should-fix` (real, later), `nit`. Fix every `blocker` you can in your branch, with the test that would have caught it, in the same branch. A `blocker` you cannot fix stays in the report unfixed and becomes a blocker ticket. A `nit` you document, you do not solve: `summary` what you observed, `anchor` where, `evidence` in what context; never hunt for its fix (you may end `evidence` with one already obvious). It becomes a box of a nits ticket that someone triages later from those words alone.
4. Write the report `.spl-asdlc/qa/<candidate>/<angle>.json` (schema in `docs/spec/06-qa-report.md`). It is mandatory even with no finding: `checked` says what you looked at, `findings` is empty, `verdict` is `go`.
5. Commit, push, open the PR to the release branch. Body: your sub-item's id, the verdict, the count of blockers fixed and unfixed. The coordinator reviews your PR as it comes, as its reviewer; answer its threads; the bridge merges it once the coordinator approved it at its head.

Your sub-item follows your PR, like any work item (BR-ASDLC-020): **In Progress** once your work starts (your claim, or your PR appearing, as a draft or not); **In Review** once the PR is ready (not draft) and its required checks are green; back **In Progress** when the coordinator requests changes at its head, until you push the fixes; **Done** when the bridge merges the PR. A PR whose title or body does not name your sub-item is neither followed nor merged; the bridge says so once on the PR, with the key to name. The candidate's overall review (`docs/agents/consolidate.md`) starts only once every angle's PR is merged.

You never write in the tracker: the bridge creates the tickets from your report.
