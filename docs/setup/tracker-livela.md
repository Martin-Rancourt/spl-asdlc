# 3b. Tracker: Livela

- **Service participant**: an `agent` participant named `spl-asdlc` (the contract, `docs/livela-contract.md`, point 4).
- **Statuses**: `to_deploy` and `post_deploy` exist for `ticket` and `bug` in the `validation` group (contract, point 1); `set_status` accepts the English names, so the mapping in `spl-asdlc.toml` is the identity.
- **Hosting**: the bridge is a `DaemonJob` of the Livela runner (contract, point 3): the runner depends on the public `spl-asdlc` crate and registers `BridgeJob`; the daemon calls it every pass with `LIVELA_SPL_ASDLC_CONFIG` (path of `spl-asdlc.toml`) and `LIVELA_JOB_BUDGET_SECONDS` (60 by default); the App's key and the tracker identity come from the environment the runner starts with. It writes directly in the local store, and the facts propagate through Livela's own sync. `todo-runner jobs` runs one pass by hand.
- **Links**: the bridge adds the PR URL as a web link on the item and finds items by that URL (contract, point 3).

**Verify**: `spl-asdlc doctor` run from the runner's environment reports `tracker: ok` and `bridge-host: livela-runner`.
