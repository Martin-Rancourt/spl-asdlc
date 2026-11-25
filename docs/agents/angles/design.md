# Angle: design

Parts: LLM instructions only; this angle ships no workflow. A project's notes add its breakpoints, its languages and its design system; they never replace this text.

On a PR, this angle is reviewed by the designer agent, like the other angles. The person's design work is not a review: it is the design step of the ticket, done on its branch before the PR is opened (`docs/method.md`, "A ticket that needs design"). By the time you review, the person has already shaped the interface; review what shipped against the design they left, and never send the PR back to them for a design pass.

## Looks for

- What the user sees and does: screens at the project's breakpoints.
- States: empty, loading, error, success.
- Copy in every language the project supports.
- Keyboard and screen-reader access, against **WCAG 2.2 AA**, the floor EN 301 549 v4.1.1 (2026) now
  shares with the EAA. Check the deltas WCAG 2.2 added over 2.1 when the screen can trigger them:
  focus not hidden by other content, touch targets of a minimum size, an alternative to a drag-only
  gesture, help placed consistently across screens, no redundant re-entry of information already
  given, and an accessible path through authentication.
- Consistency with the project's design system.

## Grading

- `blocker`: a broken flow, or a defect that carries a financial, legal, privacy, security or
  physical-harm risk.
- `should-fix`: an accessibility barrier short of a blocker, or an inconsistency with the design
  system or between screens.
- `nit`: friction, a wording or a spacing issue.

Order findings by that same consequence first: risk, then a blocker barrier, then friction — not by
a conformance level (WCAG 3's draft keeps a single conformance bar and reporting tiers, not
Bronze/Silver/Gold; still exploratory, do not adopt its tiers as this grading). At a release
candidate a `blocker` is blocking and the others are not (`review-release.md`).

## Evidence

A screenshot or a scenario in the evidence pack, with the screen, the state and the step. For an iOS
app that claims an Apple Accessibility Nutrition Label, evidence is a matrix of claimed label ×
common task (first launch, onboarding, login, purchase/subscription, settings), with the test result
for each cell: the label needs every common task to work, not just one.
