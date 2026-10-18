# Working in spl-asdlc

Read `.spl-asdlc/development.md` first: how this repository is changed, released and tested, and how its tickets move (this repository does not run the SPL-ASDLC method on itself).

- Every change is a PR to `main`, squash-merged on green CI. Repository text is in English.
- A release (merging the release-please PR, moving the `v1` tag) is published only on the owner's explicit go for that release: the bridges of real projects install it within minutes.
- After a release, move its tickets to Done (`.spl-asdlc/development.md`, "Tickets").
- Rule zero: no name of a person, product or installation that uses the method, no e-mail, home path or identifier in the repository (`scripts/check-no-instance-names.sh`, BR-ASDLC-001).
- The method itself is `docs/method.md`; the agents' protocols are `docs/agents/`.
