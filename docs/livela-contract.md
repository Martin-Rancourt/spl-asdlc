# Livela contract

What the Livela tracker must offer for its `Tracker` adapter and for hosting the bridge. Written against the Livela core as read on 2026-10-05; the conformance tests of the adapter (`docs/requirements/tracker.md`) are the acceptance of this contract. Livela implements it in its first release, independently of SPL-ASDLC.

## Already there (to be confirmed by the conformance tests, not requested again)

- Item types `task`, `bug`, `ticket`, `project`.
- Statuses with name, order, colour and group (`preparation`, `active`, `closed`), per item type.
- Relations `blocked_by` / `blocks`, `parent` / `children`, `related_to`, `duplicate_of`.
- Web links on an item (URL + title): the item ↔ PR link.
- Comments signed by an author; participants of type `human` or `agent`; tags; a lock per access point (30 minutes for an agent); short ids.
- A headless runner with a daemon that passes every N seconds (default 300) and runs deterministic code tasks; builds for Linux, macOS and Windows; the core has no platform dependency.
- The description is free Markdown: the bridge reads `- [ ]` / `- [x]` boxes in it, as it does on any tracker.

## To add

1. **A job hook in the daemon.** At each pass, call a `spl-asdlc` job (a Rust function linked into the runner, with access to the `Store` and to the network), configured by a path to `spl-asdlc.toml` and the bridge's secrets. The existing code-task library is a closed set of single-request tasks; the bridge's tick does not fit that mould, hence a distinct job. The job must be able to run with a wall-clock budget and report its outcome in the activity log.
2. **The statuses "To Deploy" and "Post Deploy"** for `ticket` and `bug`: "To Deploy" after "In Review" and before "Done"; "Post Deploy" after "To Deploy". Their group (`active` or `closed`) decides whether they appear in "what's next"; the method does not care, the user does.
3. **Find an item by the URL of one of its web links** (the inverse of adding a link). The bridge does it for every PR it sees.
4. **A participant `spl-asdlc` of type `agent`**, author of every transition and comment the bridge writes, distinct from the LLM agents.
5. **A parent item for a release** of type `ticket` (never `project`, which is excluded from "what's next"), with its angle sub-items as children; creating an item with a parent already exists.
6. **Status change by canonical name** through the API the adapter uses, with an error when the status does not exist for the item type.
7. **Listing items by tag**, and listing the children of an item.
8. **GitHub → item sync** (PR status and review comments mirrored on the item) as a Livela feature built on the `github` module of the `spl-asdlc` crate: a second job of the daemon. Not required by the method; required by the people who read items instead of PRs.

## Conformance

The adapter's conformance suite creates a release item with two angle children, links a PR URL, finds the item back by that URL, moves a child through In Review → To Deploy → Done, adds a `blocks` relation and reads it back, comments as `spl-asdlc`, lists by tag, and verifies that every write is attributed to the `spl-asdlc` participant and visible in the activity log.
