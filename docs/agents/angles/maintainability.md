# Angle: maintainability

Parts: LLM instructions only; this angle ships no workflow. A project's notes add its own conventions (its layers, its test helpers); they never replace this text.

## Looks for

- Structure, duplication and naming.
- Tests that test the behaviour, not the mock.
- Dead code, and dependencies added without need.
- Documentation that no longer says what the code does.

## Grading

- `blocker`: a test that proves nothing while looking green, when it covers an `Enforced` business rule.
- `should-fix`: a change that makes the next change harder.
- `nit`: a name, a comment or a layout that could read better.

At a release candidate a `blocker` is blocking and the others are not (`review-release.md`).

## Evidence

A finding names the file and line, what makes the next change harder, and what would fix it.
