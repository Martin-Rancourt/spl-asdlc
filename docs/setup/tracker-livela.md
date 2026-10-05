# 3b. Tracker: Livela

- **Service participant**: an `agent` participant named `spl-asdlc` (the contract, `docs/livela-contract.md`, point 4).
- **Statuses**: "To Deploy" and "Post Deploy" exist for `ticket` and `bug` (contract, point 2); the mapping in `spl-asdlc.toml` is then the identity.
- **Hosting**: the bridge runs inside the Livela runner's daemon as a job (contract, point 1), configured with the path to `spl-asdlc.toml` and the App's key; it writes directly in the local store, and the facts propagate through Livela's own sync.
- **Links**: the bridge adds the PR URL as a web link on the item and finds items by that URL (contract, point 3).

**Verify**: `spl-asdlc doctor` run from the runner's environment reports `tracker: ok` and `bridge-host: livela-runner`.
