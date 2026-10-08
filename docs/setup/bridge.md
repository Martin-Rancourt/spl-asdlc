# 4. Where the bridge runs

The bridge is the `spl-asdlc` binary (or the library linked into a tracker's runner). It needs: network access to GitHub and to the tracker, the App's id and private key (or the fallback token), the tracker's identity, and read access to the repositories' `spl-asdlc.toml` (it reads them through GitHub, so no clone is required).

| Host | How |
|---|---|
| Livela runner | `BridgeJob`, a `DaemonJob` in the runner's own code that depends on this crate by version, called every pass; see `tracker-livela.md` |
| A server or workstation | A timer or cron: `spl-asdlc tick` every five minutes; `spl-asdlc tick --dry-run` first |
| An interactive session | A skill or alias that runs `spl-asdlc tick` on demand; the latency is then the human's |

**Instance overlay**: the identities of this installation that the repository's `spl-asdlc.toml` leaves out live on this host, never in the repository (`docs/spec/08-configuration.md`, "The instance overlay"): `spl-asdlc.instance.toml` beside the configuration the bridge is given (the `--file`, or the path the Livela runner passes; `<stem>.instance.toml` beside a configuration not named `spl-asdlc.toml`), or the file named by `--instance`. It holds names, not secrets (a channel's destination and token are still `env:` references), but it is the installation's own: keep it with the host's configuration, readable by the account that runs the bridge. One overlay per configuration: with several `--file`, each reads the one beside it. A private project may have none and keep everything in its `spl-asdlc.toml`.

Secrets come from the environment or from the host's secret store; never from the repository: `SPL_ASDLC_APP_ID` and `SPL_ASDLC_APP_PRIVATE_KEY_PATH` (or the key itself in `SPL_ASDLC_APP_PRIVATE_KEY`) for the App, `SPL_ASDLC_GITHUB_TOKEN` for the personal-token fallback (`[bridge] github_identity = "token"`), `SPL_ASDLC_TRACKER_TOKEN` for the tracker, and whatever variables the `[[notify]]` references name.

**Run**
- `spl-asdlc tick --file <spl-asdlc.toml>` runs one pass over the configuration's repository; repeat `--file` for several repositories. `--instance <file>` names the overlay of a single `--file`; with several, it is refused and each file's overlay is the one beside it.
- It first prints under which names it writes, then one line per action, then a summary.
- `--dry-run` reads for real and writes nothing: it prints each action as `would do` and each write it would make.
- Exit codes: 0 when the pass is clean, 1 when an action failed or a read could not be done (each is retried at the next tick), 2 when the bridge could not start (invalid configuration, missing credentials).

**Action log**
- One JSON line per action taken or tried (when, repository, object, action, inputs, outcome), appended.
- Default place: `$XDG_STATE_HOME/spl-asdlc/<owner>-<repo>.jsonl` (or `~/.local/state/…`, or `%LOCALAPPDATA%\spl-asdlc\…`); `--log` sets another path.
- A pass with nothing to do writes nothing, not even the file.
- The log is an audit trail, not the bridge's memory: the bridge decides from what GitHub and the tracker show. Losing the file costs nothing but the history.

**Verify**: `spl-asdlc doctor --file <spl-asdlc.toml>` reports the identities it writes with, the repository it serves, the instance overlay it read (`instance overlay: ok — <path>`, or `none`), and each point of this procedure as `ok`, `missing` or `invalid`; the repository's `spl-asdlc.toml` at the default branch is judged merged with that overlay.
