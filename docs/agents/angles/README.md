# The method's angles

The method's library of review angles, optional: a project activates the ones it wants by key in its `.spl-asdlc/config.toml` (`[[review.angles]]`, `[[release.angles]]`). An angle may deliver LLM instructions, a reusable workflow, or both (`docs/reference/08-configuration.md`, "Angles"):
- its LLM instructions are `docs/agents/angles/<key>.md`, which the bridge copies into each item of the angle, at the version of spl-asdlc the project is pinned on;
- its workflow is `.github/workflows/asdlc-angle-<key>.yml`, which a project calls from its own CI, pinned on the same version as its other callers.

| Key | Reviews | Parts |
|---|---|---|
| `design` | What the user sees and does | LLM instructions |
| `maintainability` | Whether the next change stays easy | LLM instructions |
| `product` | The tickets and the business rules against what ships | LLM instructions |
| `security` | Who can do what, and what an attacker can reach | LLM instructions |

## Turning an angle on

A project turns an angle on in its own configuration; the angle's text stays the method's. For example, `security` once per release, at the release candidate:

1. Declare it under `[release]` in `.spl-asdlc/config.toml`: a `[[release.angles]]` table with `key = "security"`, and `notes = ".spl-asdlc/angles/security.md"` for the project's own notes (its surfaces, its sensitive data), which add to this text and never replace it. Leave it out of `[[review.angles]]` if it should not also run on PRs.
2. List the same key in the publication caller's `qa-angles` (`docs/setup/repository.md`, "What may follow the candidate").
3. Give it its reviewer: `assignee = "<participant>"` in that table, or in the same table of the instance overlay, never both (`docs/reference/08-configuration.md`). Unset, its items are unassigned.

The angle applies from the next candidate.

Adding or retiring an angle is a normal change of the method. A retired angle's file is removed and its key listed below, with the version that retired it: a project pinned on an older version keeps the angle, and `spl-asdlc doctor` reports it once the project's pin reaches that version.

## Retired angles

None so far: each retirement adds a row, its key and the version of spl-asdlc that removed its file.

| Key | Retired in |
|---|---|
