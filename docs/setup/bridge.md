# 4. Where the bridge runs

The bridge is the `spl-asdlc` binary (or the library linked into a tracker's runner). It needs: network access to GitHub and to the tracker, the App's id and private key (or the fallback token), the tracker's identity, and a local copy of each repository's `spl-asdlc.toml` (below).

| Host | How |
|---|---|
| Livela runner | `BridgeJob`, a `DaemonJob` in the runner's own code that depends on this crate by version, called every pass; see `tracker-livela.md` |
| A server or workstation | A timer or cron: `spl-asdlc tick` every five minutes; `spl-asdlc tick --dry-run` first |
| An interactive session | A skill or alias that runs `spl-asdlc tick` on demand; the latency is then the human's |

**The binary**: the bridge runs a published release of `spl-asdlc`, never a build from a branch, so that only code that went through a release gate runs against the repositories.
- Install: the archive of the host's target from the latest GitHub release (`spl-asdlc-v<version>-<target>.tar.gz`, or `.zip` on Windows) and its `SHA256SUMS`; verify the checksum before extracting; check that `spl-asdlc --version` prints the release's version.
- Update: before a tick, or on a slower timer, look at the latest release; install it only when its version is strictly newer than the installed one, never older (a host never downgrades by itself). Replace the binary atomically (write beside it, then rename) and keep the previous one for a one-command rollback. A failed check or download is logged and the tick runs with the binary already installed.
- A Livela runner gets the bridge with the runner's own releases (`tracker-livela.md`).

**Channels**: a host follows one channel.
- `stable`, every real project's: the published releases, as above.
- `staging`, the method's own sandbox only: the rolling prerelease `staging`, rebuilt from the method's `main` on every push. The archive is `spl-asdlc-staging-x86_64-unknown-linux-gnu.tar.gz`; its `SHA256SUMS` is checked the same way, and a `COMMIT` file in the archive names the commit built.
- The update installs a staging build whenever that commit differs from the installed one: staging has no version order. A flow bug is caught on the sandbox before a release, and its fix reaches the sandbox as soon as it is merged. Never put a real project on `staging`.

**The configuration**: `spl-asdlc tick --file <path>` reads a local file. Keep it current with the repository's default branch, for example a clone of the repository pulled (`git pull --ff-only`) before each tick, with the instance overlay beside it (ignored by git). `doctor` judges the file at the default branch merged with the overlay, so a stale local copy shows there. The file must stay readable by the installed release: a key that release does not know makes it refuse the whole file, so a new key enters the repository's file only once a release that knows it is installed (`docs/agents/implement.md`).

**Instance overlay**: the identities of this installation that the repository's `spl-asdlc.toml` leaves out live on this host, never in the repository (`docs/spec/08-configuration.md`, "The instance overlay"): `spl-asdlc.instance.toml` beside the configuration the bridge is given (the `--file`, or the path the Livela runner passes; `<stem>.instance.toml` beside a configuration not named `spl-asdlc.toml`), or the file named by `--instance`. It holds names, not secrets (a channel's destination and token are still `env:` references), but it is the installation's own: keep it with the host's configuration, readable by the account that runs the bridge. One overlay per configuration: with several `--file`, each reads the one beside it. A private project may have none and keep everything in its `spl-asdlc.toml`.

Secrets come from the environment or from the host's secret store; never from the repository: `SPL_ASDLC_APP_ID` and `SPL_ASDLC_APP_PRIVATE_KEY_PATH` (or the key itself in `SPL_ASDLC_APP_PRIVATE_KEY`) for the App, `SPL_ASDLC_GITHUB_TOKEN` for the personal-token fallback (`[bridge] github_identity = "token"`), `SPL_ASDLC_TRACKER_TOKEN` for the tracker, and whatever variables the `[[notify]]` references name.

**Run**
- `spl-asdlc tick --file <spl-asdlc.toml>` runs one pass over the configuration's repository; repeat `--file` for several repositories. `--instance <file>` names the overlay of a single `--file`; with several, it is refused and each file's overlay is the one beside it.
- It first prints under which names it writes, then one line per action, then a summary.
- `--dry-run` reads for real and writes nothing: it prints each action as `would do` and each write it would make.
- One tick at a time per repository: a tick that finds another one running on the same repository (a timer and a run by hand overlapping) does nothing, says `a tick is already running`, and exits 0. The lock is an empty file beside the action log, `<log>.jsonl.lock`, held for the tick's duration and released by the system when the process ends, even on a crash (`flock`; Windows has none, so there the host's scheduler must not overlap two ticks). A dry run takes no lock.
- Exit codes: 0 when the pass is clean, 1 when an action failed or a read could not be done (each is retried at the next tick), 2 when the bridge could not start (invalid configuration, missing credentials).

**Action log**
- One JSON line per action taken or tried (when, repository, object, action, inputs, outcome), appended.
- Default place: `$XDG_STATE_HOME/spl-asdlc/<owner>-<repo>.jsonl` (or `~/.local/state/…`, or `%LOCALAPPDATA%\spl-asdlc\…`); `--log` sets another path.
- A pass with nothing to do writes nothing, not even the file (the lock file beside it apart, which stays empty).
- On unix the log is private to the bridge's account: the file is created `0600` and a directory the bridge creates for it `0700`; an existing log file with wider permissions is narrowed to `0600` at the next write. An existing directory is left as it is.
- The log is an audit trail, not the bridge's memory: the bridge decides from what GitHub and the tracker show. Losing the file costs nothing but the history.

**Verify**: `spl-asdlc doctor --file <spl-asdlc.toml>` reports the identities it writes with, the repository it serves, the instance overlay it read (`instance overlay: ok — <path>`, or `none`), what reviews a PR (`PR review: ok — none — …` for a project with no PR review), and each point of this procedure as `ok`, `missing` or `invalid`; the repository's `spl-asdlc.toml` at the default branch is judged merged with that overlay.
