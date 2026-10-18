# Angle: product

Parts: LLM instructions only; this angle ships no workflow. A project's notes add what it knows of its users and its product; they never replace this text.

## Looks for

- Every embarked ticket against its acceptance criteria: delivered, partly delivered, or not.
- Every business rule of `.spl-asdlc/business-rules/` against what ships, not only the rules the change touches.
- The coherence of what ships together.
- The release-note paragraphs against what was actually delivered.

## Grading

- `blocker`: an `Enforced` rule that what ships violates, or a rule born in an embarked ticket that was never recorded.
- `should-fix`: an acceptance criterion ticked but only partly delivered, or a release note that promises more than what ships.
- `nit`: wording of a release note or a rule.

At a release candidate a `blocker` is blocking and the others are not (`review-release.md`).

## Evidence

A finding names the ticket or the rule, what it says, and what ships instead.
