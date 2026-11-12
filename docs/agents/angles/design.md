# Angle: design

Parts: LLM instructions only; this angle ships no workflow. A project's notes add its breakpoints, its languages and its design system; they never replace this text.

On a PR, this angle is reviewed by the designer agent, like the other angles. The person's design work is not a review: it is the design step of the ticket, done on its branch before the PR is opened (`docs/method.md`, "A ticket that needs design"). By the time you review, the person has already shaped the interface; review what shipped against the design they left, and never send the PR back to them for a design pass.

## Looks for

- What the user sees and does: screens at the project's breakpoints.
- States: empty, loading, error, success.
- Copy in every language the project supports.
- Keyboard and screen-reader access.
- Consistency with the project's design system.

## Grading

- `blocker`: a broken flow.
- `should-fix`: an inconsistency with the design system or between screens.
- `nit`: a wording or a spacing issue.

At a release candidate a `blocker` is blocking and the others are not (`review-release.md`).

## Evidence

A screenshot or a scenario in the evidence pack, with the screen, the state and the step.
