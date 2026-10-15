# Refine a ticket

**Input**: a ticket in Backlog. **Output**: the same ticket, still in Backlog, with a complete contract and marked refined, or a comment that lists what the owner must decide.

1. Read the ticket and everything it links. Identify the one outcome it asks for.
2. Write or fix the **Goal**: one paragraph, what is true when the ticket is done. Not a task list.
3. Write the **Non-goals** (recommended): what a reader could expect and will not get, so that the implementer does not wander.
4. Write the **Acceptance criteria** as checkboxes. Each is verifiable by reading the code, running a command, or looking at a screen. Each names its proof when it is not obvious. No box for a note; a note is a bullet.
5. Write the **Business rules** section: the new global invariants this ticket introduces, each as a bullet that will become a file of `.spl-asdlc/business-rules/` with a test, or "None.".
6. Write the **Release note** snippet (recommended): one paragraph for the person who uses the product; empty only for an internal change.
7. Run `spl-asdlc ticket check <item>` (or apply its rules by hand: a goal, at least one acceptance box, a business-rules section). Fix what it reports.
8. If a decision belongs to the owner, do not guess: write a comment that lists the questions, assign the ticket to the owner with the hold label, and stop. Otherwise mark the ticket refined (the `Refined` label in Linear, the refined state in Livela) and leave it in Backlog.

You never move the ticket to Ready: a person does, when agents may take it. Never write an acceptance criterion the implementer cannot prove, and never split a ticket to make it look complete.
