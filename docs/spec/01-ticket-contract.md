# 01 — The ticket contract

## Invariants

1. **A ticket is implementable only with a contract**: a **Goal** (what is true once the ticket is done), **Non-goals** (what the ticket deliberately leaves out), **Acceptance criteria** (verifiable checkboxes), and **Business rules** (new global rules the ticket introduces, or the sentence that declares there are none).
2. **The contract is verified before the ticket is taken.** Refinement is the step that turns a draft into a contract. A ticket without a goal or without at least one acceptance-criterion checkbox is not served to an implementer; it goes back to refinement, assigned to whoever must complete it.
3. **The contract is every checkbox of the description**, under any heading, plus the checkboxes of the tickets it duplicates. A note that does not commit is written as a plain bullet, never as a checkbox. Content inside fenced code blocks is ignored.
4. **A ticket with checkboxes and none ticked is not reviewed and not merged.** Before a PR is reviewed, its author must have declared what the PR delivers by ticking at least one box.
5. **A section `## Post deploy`** (optional) holds the boxes that can only be true once the release is deployed. They are part of the contract and keep the ticket from Done, but a release does not count them: a ticket with only those boxes open goes to Post Deploy at closure, not back to In Progress, and to Done at the tick that finds them all checked.
6. **The release-note snippet is written at refinement.** A short paragraph for a human reader that says what the ticket changes for them. It lives in the ticket under `## Release note` and the release note of a cycle is built from these snippets.
7. **A PR is linked to exactly one ticket; a ticket has one PR by default.** A ticket has several PRs only for a real reason, which its implementer records in the ticket's comments; each of them is independently deployable, and the ticket's state follows all of them (`04-pr-review.md` §10). The link is a tracker attachment or a URL on the item, resolved by the bridge from the PR; free text in titles and bodies is never parsed to find a ticket.

## The template

```markdown
## Goal
<one paragraph: what is true when this is done>

## Non-goals
- <what is left out, and why if it is not obvious>

## Acceptance criteria
- [ ] <verifiable statement>
- [ ] <verifiable statement>

## Business rules
<"None." or one bullet per new rule, each with its id once promoted to the registry>

## Release note
<one paragraph for the human reader; may be empty for an internal change>
```

## The refinement gate

The bridge offers `spl-asdlc ticket check <item>`: it reads the description and answers *contract complete* or lists what is missing (no goal, no acceptance-criterion box, business-rules section absent). Any runtime may call it before serving a ticket. CI never calls it (CI reads no tracker, `00`): the bridge holds a PR whose ticket's contract is incomplete and says so once on the PR (`04` §2). The judgement is deterministic: it reads headings and checkboxes, never the quality of the prose.

## Vocabulary

- **Contract**: the set of checkboxes of a ticket and its duplicates.
- **Open box / ticked box**: `- [ ]` / `- [x]` in the description.
- **No criteria declared**: a ticket with no checkbox at all; it fails the Done gate like an open box does.
