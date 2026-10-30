# 01 — The ticket contract

## Invariants

1. **A ticket is implementable only with a contract**: a **Goal** (what is true once the ticket is done), **Acceptance criteria** (verifiable checkboxes), and **Business rules** (new global rules the ticket introduces, or the sentence that declares there are none). **Non-goals** (what the ticket deliberately leaves out) are recommended, not required.
2. **The contract is verified before the ticket is taken.** Refinement is the step that turns a draft into a contract. A ticket without a goal or without at least one acceptance-criterion checkbox is not served to an implementer; it goes back to refinement, assigned to whoever must complete it.
3. **The contract is every checkbox of the description**, under any heading, plus the checkboxes of the tickets it duplicates. A note that does not commit is written as a plain bullet, never as a checkbox. Content inside fenced code blocks is ignored.
4. **Every box is checked before a PR is ready, except under `## Post deploy`.** The bridge returns a PR with unfinished ticket work to draft, its ticket to In Progress, listing the missing lines. A line linked to an unfinished task child counts as unchecked. Marking the PR ready again retries the check.
5. **A section `## Post deploy`** (optional) holds the boxes that can only be true once the release is deployed. They are part of the contract and keep the ticket from Done, but a release does not count them: a ticket with only those boxes open goes to Post Deploy at closure, not back to In Progress, and to Done at the tick that finds them all checked.
6. **The release-note snippet is recommended at refinement**, never required. A short paragraph for a human reader that says what the ticket changes for them. It lives in the ticket under `## Release note` and the release note of a cycle is built from these snippets.
7. **A PR is linked to exactly one ticket; a ticket ships in one PR.** A technical constraint requiring several PRs is handled by sibling tickets linked through `blocked by`, each with its own PR. The link is a tracker attachment or a URL on the item, resolved by the bridge from the PR; without one, the ticket id in the PR's branch (`<type>/<ticket-id>-<description>`) or at the end of its title names the ticket, compared without regard to case (BR-ASDLC-036), and the bridge then lays the tracker's link. Free text in titles and bodies is never parsed to find a ticket, and an id no item of the project has names none. The items the bridge links to the PR for its review (`04-pr-review.md` §3) are never its ticket: an item is dropped by its marker, and an item whose parent is another item linked to the same PR is dropped whatever its description says, since an agent editing it may have mangled the marker.

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
