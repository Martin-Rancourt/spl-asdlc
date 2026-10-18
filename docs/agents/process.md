# How to work in a project that runs SPL-ASDLC

This page is the procedure every agent follows in a project that runs SPL-ASDLC: how a ticket is refined and implemented, how a PR is titled, reviewed and merged, how the ticket moves, who closes it and when, and what you never do by hand. It is the same in every project.

SPL-ASDLC delivers this page into the project's repository at `.spl-asdlc/process.md`, at the version the project's workflows pin. Read it there, in your clone. Never edit it: a method check refuses every PR whose copy differs from the pinned version's. The project's own instructions (how to build and test, the folders that need care) are in its agent file, `AGENTS.md`, around this page. When they contradict this page, this page wins.

## One procedure: read the configuration first

There is one procedure. A project's configuration only says which optional steps are present; it never changes how a step works. Before you start, read `.spl-asdlc/config.toml` and skip the steps it leaves out:

| Step | Present when | Absent when |
|---|---|---|
| PR review | `[review]` has `[[review.angles]]` tables | `angles = []` under `[review]`: a PR merges on its CI |
| Release QA | `[release]` has `[[release.angles]]` tables | `angles = []` under `[release]`: a release's GO rests on its CI |
| The release cycle | always | never |

Some keys change a detail, never the procedure: `tracker.no_deploy_label` marks a ticket with nothing to deploy (Done at its merge); `review.require_up_to_date` asks a PR to be up to date with its base before it merges; `release.auto_candidate` lets the bridge cut the next candidate. Who is assigned what may live outside the repository, on the bridge's host: you do not need it, the tracker tells you what is yours.

## The ticket's statuses, and who moves them

The bridge is a deterministic program that reads GitHub and moves the tickets. You move a ticket once: to In Progress, as you start it.

| Status | Who moves the ticket there | When |
|---|---|---|
| Backlog | A person | A new ticket worth keeping. Refinement marks it refined and leaves it there. |
| Ready | A person | When agents may take it. Implementers take work from Ready only. |
| In Progress | You, the implementer | As you start it. The bridge moves it back there when changes are requested on its PR, or when a merged PR leaves boxes open. |
| In Review | The bridge | When its PR is opened (not a draft). |
| To Deploy | The bridge | At the merge, once every box is ticked and no other PR of the ticket is open: the code is on the base branch and waits for the next release. |
| Done | The bridge | At the closure of the release that ships it. A ticket with nothing to deploy (the no-deploy label) is Done at its merge. |
| Post Deploy | The bridge | At the closure, when only the boxes under `## Post deploy` remain; an urgent validation sub-task asks to verify them in production. |

**Who closes a ticket**: the bridge, at the closure of the release that ships it (at its merge when it has nothing to deploy). Never close a ticket yourself, never move it to In Review, To Deploy or Done, and never move a ticket to Ready.

## Refine a ticket

**Input**: a ticket in Backlog. **Output**: the same ticket, still in Backlog, with a complete contract and marked refined, or a comment that lists what the owner must decide.

1. Read the ticket and everything it links. Identify the one outcome it asks for.
2. Write or fix the **Goal**: one paragraph, what is true when the ticket is done. Not a task list.
3. Write the **Non-goals** (recommended): what a reader could expect and will not get, so that the implementer does not wander.
4. Write the **Acceptance criteria** as checkboxes. Each is verifiable by reading the code, running a command, or looking at a screen. Each names its proof when it is not obvious. No box for a note; a note is a bullet. A box that can only be verified in production goes under `## Post deploy`.
5. Write the **Business rules** section: the new global invariants this ticket introduces, each as a bullet that will become a file of `.spl-asdlc/business-rules/` with a test, or "None.".
6. Write the **Release note** snippet (recommended): one paragraph for the person who uses the product; empty only for an internal change.
7. Run `spl-asdlc ticket check <item>` (or apply its rules by hand: a goal, at least one acceptance box, a business-rules section). Fix what it reports.
8. If a decision belongs to the owner, do not guess: write a comment that lists the questions, assign the ticket to the owner with the hold label, and stop. Otherwise mark the ticket refined (the `Refined` label in Linear, the refined state in Livela) and leave it in Backlog.

Never write an acceptance criterion the implementer cannot prove, and never split a ticket to make it look complete.

## Implement a ticket

**Input**: a Ready ticket with a contract. **Output**: a pull request linked to the ticket, with the acceptance criteria ticked as they are met.

1. Read the contract. If it is incomplete or contradictory, stop and send the ticket back to refinement with a comment; do not implement guesses. Otherwise move the ticket to In Progress as you start (with Livela, "implement the next ticket" does it when it serves you the ticket). The bridge never does it for you: a branch or a draft PR leaves the ticket where it is.
2. Create a branch from the base the ticket targets: the default branch, or the release branch for a fix during a release cycle (a gate-fix ticket or a blocking finding's fix ticket names it, and its PR's base is that branch). Name the branch after the ticket. Never stack it on another PR's branch: with a squash merge, a stacked PR carries the other PR's diff into the base under its own merge, and the bridge never sees that other PR, nor its ticket, merged.
3. Work in small commits with Conventional Commit messages. Add the tests the acceptance criteria imply. A business rule the ticket introduces gets its file in `.spl-asdlc/business-rules/` and its test in the same branch: the bridge does not merge a PR whose ticket states a new rule without a rule file.
   - Never add a key to `.spl-asdlc/config.toml` that the bridge installed for the project does not know: it refuses the whole file and stops serving the repository. A new key enters that file once a release that knows it is installed.
4. Tick each acceptance criterion in the ticket when it is met, not before, not at the end. Tick a box by changing its `[ ]` to `[x]` and nothing else. Never remove or rewrite a marker line the bridge put in a ticket (`<!-- spl-asdlc:… -->`, or `spl-asdlc:…` as inline code): the bridge finds the ticket by it.
5. Open the PR when the work is ready, never earlier. **Title**: a Conventional Commit with the ticket id at the end, such as `fix(scope): summary (ABC-123)`; the PR-title check refuses a title that is not a Conventional Commit. **Body**: the acceptance criteria covered, how to verify, anything the reviewers must know. Link the PR to the ticket the way the tracker does it (attachment, or the item URL in the body). Opening the PR puts the ticket In Review.
6. Keep CI green. A red check is yours to fix before anyone reviews.
7. With PR review present, the review starts once CI is green: angle reviewers post findings on the PR, and a coordinator answers them, applies the retained fixes on your branch and approves. When the coordinator requests changes, the ticket comes back to you: fix on the same branch, answer the threads, push. With no PR review, the PR merges on its CI.
8. The bridge merges the PR (squash, unless the configuration says otherwise) once CI is green, the review is done and approved when the PR has one, and the branch is up to date when the project requires it. You never merge it.
9. One PR per ticket is the default, with as many commits as needed. Open a second PR for the same ticket only for a real reason, typically a deployment that must happen before the rest, and record that reason in a comment on the ticket before you open it. A later PR also starts from the base (step 2), once the earlier one is merged. Every PR must be deployable on its own: once merged it may ship at any time, even if the ticket is not done.
10. A merged PR that leaves boxes open sends the ticket back to you (In Progress): carry on, tick what each PR delivers, and open the next PR, until every box is ticked.

## Review, consolidate, release QA

These jobs come as items the bridge creates and serves to their reviewer or coordinator: an angle sub-item of a PR or of a release candidate, a consolidation item. Each item carries its whole protocol under its `## Protocol` section, at the version that created it. Follow that protocol; it says what to leave behind (a review on the PR, a report file, an approval) and what closes the item. Never move such an item's status: the bridge does, from what you leave on GitHub.

## The release cycle

Every project has one. It takes the tickets in To Deploy, already on the default branch, to production.

1. **Cut.** A person dispatches the cut workflow on the default branch: it creates the release branch, writes the version and the CHANGELOG, and tags the first candidate. The bridge opens the release issue, assigned to the owner.
2. **Release CI.** The candidate's tag runs the project's release suites. When it is red, the bridge opens an urgent gate-fix ticket in Ready: implement it as any ticket, with the release branch as the base.
3. **QA**, when present. Angles review the candidate, each on its own angle branch with a report; the coordinator reviews each angle PR, then the whole. A blocking finding becomes an urgent fix ticket on the release branch, a non-blocking one a ticket for later.
4. **GO.** It is computed by the bridge from the facts, never declared: the latest candidate's CI green, every QA angle done, no blocking finding open. The bridge then tags and publishes the release.
5. **Publication.** The project's publish workflow deploys the release and merges the release branch back into the default branch.
6. **Closure.** The bridge judges each ticket of the release: every box ticked, Done; only `## Post deploy` boxes left, Post Deploy; anything else open, back to In Progress.

Each fix merged to the release branch makes the next candidate: the coordinator cuts it as its protocol says, a person does, or the bridge does with `release.auto_candidate = true`. Work merged to the default branch during a cycle goes into the next release.

## Drafts are out of the flow

The bridge ignores a draft PR entirely: it moves no ticket for it, opens no review for it, never merges it, does not read its CI. No CI runs on a draft. A person takes a PR out of the flow by putting it in draft, and back in by marking it ready.

## What you never do by hand

Never, in any project:

- edit `.spl-asdlc/process.md`: the method changes it, through an update of the version the project pins;
- stack a branch on another PR's branch, or open a PR before the work is ready;
- remove or rewrite a marker line the bridge put in an item;
- write a review's findings in the tracker instead of on the PR, unless your item's protocol says so.

A person may take any step of the flow by hand, in GitHub or in the tracker, and the bridge takes the result as done. These steps are the bridge's or a person's, never yours unless the owner gives you an explicit go for that one step:

- merge a PR, or push to the default branch or a release branch;
- move a ticket to Ready, In Review, To Deploy, Post Deploy or Done, or close it;
- move the status of an item the bridge created (angle sub-item, consolidation item, release issue);
- cut a release, cut a candidate (but the one your consolidation protocol asks of you), tag or publish a release, or declare the GO.
