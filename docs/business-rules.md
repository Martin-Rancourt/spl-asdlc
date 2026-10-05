# SPL-ASDLC business rules

Format: one row per rule. A rule is `Enforced` only when the named test runs in CI; until then it is `Proposed`. Ids are `BR-ASDLC-NNN` and are never reused.

| ID | Rule | Evidence | Test | Status |
|---|---|---|---|---|
| BR-ASDLC-001 | **The repository carries no instance data.** No name of a product, agent, person, organisation member or installation that uses the method appears anywhere in the repository (code, docs, tests, workflows, fixtures), and no identifier of an installation does either: e-mail addresses, `/home/<user>` or `/Users/<user>` paths, 9+-digit account or chat ids, hexadecimal uids, GitHub App bot logins. Product values that hold for every install (the repository slug of this product, its own crate name) are allowed. | The method must be reusable by anyone, and publishable, without a scrub. | `scripts/check-no-instance-names.sh`: scans every tracked file for the identifier forms above, and for the words listed in the environment variable `SPL_ASDLC_FORBIDDEN_WORDS` (comma-separated; kept outside the repository, in a CI secret or a local untracked file, precisely so that the list itself never enters the repository). Zero hits, no baseline, no exemption list. | Enforced |
