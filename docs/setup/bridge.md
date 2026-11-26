# 4. Where the bridge runs

The bridge is the `spl-asdlc` binary (or the library linked into a tracker's runner). It needs: network access to GitHub and to the tracker, the App's id and private key (or the fallback token), the tracker's identity, and a local copy of each repository's `.spl-asdlc/config.toml` (below).

| Host | How |
|---|---|
| Livela runner | `BridgeJob`, a `DaemonJob` in the runner's own code that depends on this crate by version, called every pass; see `tracker-livela.md` |
| A server or workstation | A timer or cron: `spl-asdlc tick` at the interval the host chooses (every five minutes, for example); `spl-asdlc tick --dry-run` first |
| An interactive session | A skill or alias that runs `spl-asdlc tick` on demand; the latency is then the human's |

**The binary**: the bridge follows the stable channel, the published releases of `spl-asdlc`, never a build from a branch, so that only code that went through a release gate runs against the repositories.
- Install: the archive of the host's target from the latest GitHub release (`spl-asdlc-v<version>-<target>.tar.gz`, or `.zip` on Windows) and its `SHA256SUMS`; verify the checksum before extracting; check that `spl-asdlc --version` prints the release's version.
- Update automatically with `spl-asdlc runner --file <host>/spl-asdlc-runner.toml` (below). Before every pass it installs the highest stable version of the requested major, including minor releases. It searches releases of that major even when a newer major has been published. Drafts, prereleases, and versions older than or equal to the installed version are never installed.
- The host configuration explicitly selects the major. Changing it is the user's action to authorize a new major; publication alone never changes it. A dependent host supplies its own chosen major through `spl-asdlc update --major <N>` before launching the installed binary. The library itself never updates its host.

**Standalone runner**: install the binary in a directory writable by the runner's account, with `tar` on PATH (GNU tar on Linux, the system bsdtar on macOS/Windows). Keep a host configuration separate from repository configuration:

```toml
# spl-asdlc-runner.toml — changing major authorizes that major's stable releases.
major = 1
files = ["/srv/project/.spl-asdlc/config.toml"]
interval_seconds = 300
```

Run `spl-asdlc runner --file <host>/spl-asdlc-runner.toml` under the host's service manager. Paths in `files` are relative to its working directory. The configuration is reread before each pass. The supervisor invokes the installed executable for updates and ticks, so the next pass runs the updated code. A failed check, download, checksum, extraction or version verification logs an error and the tick uses the installed binary. For a timer or another supervisor, run `spl-asdlc update --major <N>` before each tick instead.

The updater verifies `SHA256SUMS` and the extracted binary's `--version`, takes a lock beside the binary, and stages in a private directory on the same filesystem. On Unix the final rename is atomic; on Windows it moves the mapped executable aside before installing, restoring it if installation fails. It keeps the previous version as `<binary>.previous-<version>` for a manual rollback (stop the service and move that file back). Windows hosts must serialize their updater invocations. A rollback is a manual action; auto-update never downgrades, even when the requested major is lower than the installed one. Restart the supervisor after an explicit major upgrade so its supervision code also moves to that major.

Every stable release's publication workflow also publishes the crate from the same tag, after verifying that `Cargo.toml` carries that version. Existing versions are skipped on reruns; a missing registry token fails publication rather than silently skipping it. Dependents monitor crates.io; the bridge sends no repository dispatch.

**The cut's binary**: the release cut runs the binary too, for `release prepare`: its caller names the release (`spl-asdlc-version`: `latest`, a minor line `v<X.Y>` for its newest patch, or a release tag), and the cut downloads that archive and checks it against `SHA256SUMS` the same way (`repository.md`, "The binary the cut runs"). Keep the cut on the release its bridge runs.

**The configuration**: `spl-asdlc tick --file <path>` reads a local file. Keep it current with the repository's default branch, for example a clone of the repository pulled (`git pull --ff-only`) before each tick, with the instance overlay beside it (ignored by git). `doctor` judges the file at the default branch merged with the overlay, so a stale local copy shows there. The file must stay readable by the installed release: a key that release does not know makes it refuse the whole file, so a new key enters the repository's file only once a release that knows it is installed (`docs/agents/process.md`, "Implement a ticket" step 3).

**Instance overlay**: the identities of this installation that the repository's `.spl-asdlc/config.toml` leaves out live on this host, never in the repository (`docs/reference/08-configuration.md`, "The instance overlay"): `config.instance.toml` beside the configuration the bridge is given (the `--file`, or the path the Livela runner passes; `<stem>.instance.toml` beside a configuration of any name), or the file named by `--instance`. It holds names, not secrets (a channel's destination and token are still `env:` references), but it is the installation's own: keep it with the host's configuration, readable by the account that runs the bridge. One overlay per configuration: with several `--file`, each reads the one beside it. A private project may have none and keep everything in its `.spl-asdlc/config.toml`.

Secrets come from the environment or from the host's secret store; never from the repository: `SPL_ASDLC_APP_ID` and `SPL_ASDLC_APP_PRIVATE_KEY_PATH` (or the key itself in `SPL_ASDLC_APP_PRIVATE_KEY`) for the App, `SPL_ASDLC_GITHUB_TOKEN` for the personal-token fallback (`[bridge] github_identity = "token"`), `SPL_ASDLC_TRACKER_TOKEN` for the tracker, and whatever variables the `[[notify]]` references name.

**Run**
- `spl-asdlc tick --file <checkout>/.spl-asdlc/config.toml` runs one pass over the configuration's repository; repeat `--file` for several repositories. `--instance <file>` names the overlay of a single `--file`; with several, it is refused and each file's overlay is the one beside it.
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

**Verify**: `spl-asdlc doctor --file <checkout>/.spl-asdlc/config.toml` reports the identities it writes with, the repository it serves, the instance overlay it read (`instance overlay: ok — <path>`, or `none`), what reviews a PR (`PR review: ok — none — …` for a project with no PR review), and each point of this procedure as `ok`, `missing` or `invalid`; the repository's `.spl-asdlc/config.toml` at the default branch is judged merged with that overlay.

For reactions within seconds without exposing the host, opt in to the
[webhook relay and `watch` host](webhook-relay.md). Polling stays enabled.
