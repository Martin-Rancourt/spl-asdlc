# 3a. Tracker: Linear

- **Service identity**: a dedicated member of the workspace named after the product's service identity (the reference installation names it `spl-asdlc`), with an API key stored where the bridge runs. Every transition and comment the bridge writes is attributed to it, never to a person or to an LLM agent.
- **Statuses**: the team's workflow must contain states whose names map to the canonical ones (`docs/spec/03-vocabulary.md`); the mapping is in `spl-asdlc.toml` `[tracker.statuses]`. "To Deploy" and "Post Deploy" are usually created for this purpose.
- **Labels**: `qa-finding`, `blocked-by-human`; the bridge creates `release/<product>-v<version>` labels as it goes.
- **PR ↔ ticket link**: Linear's GitHub integration attaches a PR to an issue when the branch name or the PR body carries the issue id; the bridge reads attachments by PR URL. Turn **off** any Linear automation that moves issues on PR events: the bridge is the only writer of statuses.
- **Team key**: in `spl-asdlc.toml` `[tracker] team`.

**Verify**: `spl-asdlc doctor` reports `tracker: ok` with the identity's name, the mapped statuses and the labels found.
