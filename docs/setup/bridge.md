# 4. Where the bridge runs

The bridge is the `spl-asdlc` binary (or the library linked into a tracker's runner). It needs: network access to GitHub and to the tracker, the App's id and private key (or the fallback token), the tracker's identity, and read access to the repositories' `spl-asdlc.toml` (it reads them through GitHub, so no clone is required).

| Host | How |
|---|---|
| Livela runner | `BridgeJob`, a `DaemonJob` of the runner, called every pass; see `tracker-livela.md` |
| A server or workstation | A timer or cron: `spl-asdlc tick` every five minutes; `spl-asdlc tick --dry-run` first |
| An interactive session | A skill or alias that runs `spl-asdlc tick` on demand; the latency is then the human's |

Secrets come from the environment (`SPL_ASDLC_APP_ID`, `SPL_ASDLC_APP_PRIVATE_KEY_PATH`, `SPL_ASDLC_TRACKER_TOKEN`) or from the host's secret store; never from the repository.

**Verify**: `spl-asdlc doctor` without `--repo` reports the identities it would write with and the repositories it would serve.
