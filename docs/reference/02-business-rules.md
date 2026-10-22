# 02 — Business rules

## Invariants

1. **A business rule is a global, permanent invariant of the project.** Unlike an acceptance criterion, which lives and dies with its ticket, a rule applies to the whole current state of the application. If the existing code violates a new rule, fixing it is in scope of the ticket that introduces the rule.
2. **Rules live in the repository**, in the registry `.spl-asdlc/business-rules/`, one file per rule named after its stable id, `BR-<PRODUCT>-<NNN>.md`; an id is never reused. The product prefix is two to six capital letters. The place is fixed by the method, never configured. One file per rule means two PRs that touch different rules never conflict.
3. **A rule is enforced only by a named test that runs in CI.** The rule's file names the test. A rule without a test is `Proposed`; only `Enforced` rules block. `spl-asdlc doctor` checks, on its `business-rule tests` line, that each test an `Enforced` rule cites exists in the repository at the default branch ("The rule file" below says how it reads them). It does not run them: that is CI's.
4. **A rule born in a ticket is promoted to the registry in the same PR.** The bridge refuses to merge a PR whose ticket states a new rule when the PR adds or changes no rule file (`.spl-asdlc/business-rules/<ID>.md`; the index alone does not count).
5. **A rule cited by id must exist** in the registry of the product it names, read at the ref being judged: the PR head at merge time, the release tag at closure. The registry is read through the Git host, never from a working tree: the guard reads the directory's listing, and an id is registered when its file `<ID>.md` is there. A directory absent at a ref that exists is a registry with no rule.
6. **The guard fails closed.** An unreadable registry keeps the PR waiting and, at closure, leaves the ticket in its state and reports the rule as unverified. A ticket that neither cites nor states a rule is judged without reading the registry.
7. **Declaring the absence of a rule is explicit**: the Business rules section contains a sentence that says there is none (`None.`, `No new rule.`, `N/A`). An empty section is not a declaration. A sentence that merely begins with "No" but goes on to state something is a rule.

## The rule file

`.spl-asdlc/business-rules/<ID>.md`, one per rule; `docs/setup/templates/business-rule.md` is an example to copy.

```markdown
# BR-ACME-001 — Every exported file carries its signature.

Status: Enforced

## Rule

The detail of the invariant.

## Evidence

Why it exists; the incident or decision that created it.

## Test

The test name(s) that enforce it, as CI reports them.
```

| Part | Content |
|---|---|
| Title, the first line | `# <ID> — <the rule in one sentence>`; the id is the file's name |
| `Status:` line | `Proposed` or `Enforced` |
| `## Rule` | The invariant in detail |
| `## Evidence` | Why it exists; the incident or decision that created it |
| `## Test` (or `## Tests`) | The test name(s) that enforce it, as CI reports them |

Only the file's name registers an id; an id mentioned in another file vouches for nothing. A file may carry more sections (domain, owner decision); the parts above are the contract, and none is empty. `spl-asdlc doctor` reads every file of the directory and names each one that breaks the format by its path. Beside the rules the directory may hold a `README.md`, an index for people that the method never reads.

`doctor`'s `business-rule tests` line reads the backticked names of each `Enforced` rule's Test section, in any language:

- A repository path (`tests/export.rs`, `apps/web/__tests__/export.test.ts`, `scripts/check.sh`) is a file: it must exist.
- A test name (`a_snake_case_name`, or a nested name `export > signs the file`) is looked for in the file named beside it, in the same paragraph or list item; a list item that names no file takes the file of the paragraph before it.
- A Rust module path (`export::tests::signs_the_file`) is looked for in the file its modules lead to (`src/export/tests.rs`, `src/export/tests/mod.rs`, `src/export.rs`, then the same under `tests/`). A `*` last segment (`export::tests::*`) only needs the file.
- A name is found when its last part (after the last `::` or ` > `) appears as written in the file.

The line is `invalid` when a cited file is absent or a name is not in its file, and names each rule and what is absent. A name whose file cannot be located is counted as `not located`, never a failure, and any other token (a job, a task, a word) is ignored. A file that cannot be read makes the line `missing`.

## Transverse rules of the method

The method has its own rules, which hold for every project that uses it. They are numbered `SR-<NNN>` and live in `docs/reference/02-business-rules.md` of this repository, under this heading, in the table below. The first ones, carried over from the previous implementation:

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
| SR-010 | Every `Enforced` rule names a test that runs in CI; `doctor`'s `business-rule tests` line checks that each test it names exists. |
| SR-011 | A skipped CI step is never read as a success, except a job the caller lists in `required-check`'s `may-skip`; the method's guards may never be listed there. |
| SR-012 | Every actor that writes to the Git host does so under its own traceable identity. |
| SR-013 | The contract of a ticket is every checkbox of its description and of its duplicates. |
| SR-014 | A ticket that can only move forward through a human is assigned to that human, with the escalation label (`release.escalation_label`) and a comment that says what is expected and to whom it returns. |
| SR-015 | A post-deploy validation is an operator action, not a PM decision: its sub-task is created in Post Deploy, assigned to the operator, and never receives an escalation label. |
