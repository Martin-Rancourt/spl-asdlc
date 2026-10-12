# 02 — Business rules

## Invariants

1. **A business rule is a global, permanent invariant of the project.** Unlike an acceptance criterion, which lives and dies with its ticket, a rule applies to the whole current state of the application. If the existing code violates a new rule, fixing it is in scope of the ticket that introduces the rule.
2. **Rules live in the repository**, in a registry `docs/business-rules.md`, one row per rule, with a stable id `BR-<PRODUCT>-<NNN>` that is never reused. The product prefix is two to six capital letters.
3. **A rule is enforced only by a named test that runs in CI.** The registry row names the test. A rule without a test is `Proposed`; only `Enforced` rules block.
4. **A rule born in a ticket is promoted to the registry in the same PR.** The bridge refuses to merge a PR whose ticket states a new rule when the PR does not touch the registry.
5. **A rule cited by id must exist** in the registry of the product it names, read at the ref being judged: the PR head at merge time, the release tag at closure. The registry is read through the Git host, never from a working tree.
6. **The guard fails closed.** An unreadable registry keeps the PR waiting and, at closure, leaves the ticket in its state and reports the rule as unverified. A ticket that neither cites nor states a rule is judged without reading the registry.
7. **Declaring the absence of a rule is explicit**: the Business rules section contains a sentence that says there is none (`None.`, `No new rule.`, `N/A`). An empty section is not a declaration. A sentence that merely begins with "No" but goes on to state something is a rule.

## The registry format

| Column | Content |
|---|---|
| `ID` | `BR-<PRODUCT>-<NNN>` |
| `Rule` | The invariant, one bold sentence then the detail |
| `Evidence` | Why it exists; the incident or decision that created it |
| `Test` | The test name(s) that enforce it, as the CI reports them |
| `Status` | `Proposed` or `Enforced` |

Only ids in the first cell of a table row count as registered. Rows may carry more columns (domain, owner decision); the five above are the contract.

## Transverse rules of the method

The method has its own rules, which hold for every project that uses it. They are numbered `SR-<NNN>` and live in `docs/spec/02-business-rules.md` of this repository, under this heading, with the same columns. The first ones, carried over from the previous implementation:

| ID | Rule |
|---|---|
| SR-001 | No secret and no personal data in code, fixtures or packaged templates. |
| SR-002 | A PR is linked to exactly one ticket; a PR without a linked ticket is never merged automatically. |
| SR-003 | PR titles follow Conventional Commits; the PR ↔ ticket link is carried by a dedicated mechanism, never by the title. |
| SR-004 | No merge bypasses the required checks. |
| SR-005 | A merged PR's branch is deleted; a release branch merged back into `main` is deleted after the merge. |
| SR-006 | A release-candidate tag is immutable once pushed: the gate judges a frozen ref. |
| SR-007 | The GO of a release is computed, never declared (or taken on themselves by a person who tags and publishes the release, spec 09). |
| SR-008 | The angles of a release QA are configured per project, never a shared constant. |
| SR-009 | A release QA has a configurable number of RC passes; when they are exhausted with blockers left, a human decides. |
| SR-010 | Every `Enforced` rule names a test that runs in CI. |
| SR-011 | A skipped CI step is never read as a success. |
| SR-012 | Every actor that writes to the Git host does so under its own traceable identity. |
| SR-013 | The contract of a ticket is every checkbox of its description and of its duplicates. |
| SR-014 | A ticket that can only move forward through a human is assigned to that human, with a hold label and a comment that says what is expected and to whom it returns. |
