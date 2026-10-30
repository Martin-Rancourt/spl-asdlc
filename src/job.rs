//! One pass of the bridge for a host (`docs/reference/07-the-bridge.md`
//! invariant 1): the Livela runner's daemon job, a timer, a shell. The host
//! gives the configuration file, the tracker and a time budget; the pass
//! reads its secrets from the environment, runs one tick, appends the action
//! log, and answers ok or failed with one line for the host's job log. A
//! host that embeds the crate gives the repository instead of a file: the
//! configuration is read at its default branch, merged with the instance
//! overlay the host passes as a value ([`pass_from_repository`]).
//!
//! A pass that does nothing writes nothing: no fact, no log line, no file
//! (invariant 7) but the empty lock file beside the action log, which keeps
//! a second pass on the same repository from running while one runs
//! (invariant 8): that second pass does nothing and says so. Only the host
//! records a failed pass.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::action_log::ActionLog;
use crate::config::{Config, Source};
use crate::github::{GitHub, RepoSlug, Timestamp};
use crate::lock::TickLock;
use crate::notify::{Channels, Signal};
use crate::quota::Pace;
use crate::tick::{self, Tick};
use crate::tracker::Tracker;

/// The outcome of one pass, for the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassOutcome {
    pub ok: bool,
    pub message: String,
}

impl PassOutcome {
    pub fn failed(message: impl Into<String>) -> PassOutcome {
        PassOutcome {
            ok: false,
            message: message.into(),
        }
    }
}

/// What a host hands the bridge for a pass.
pub struct Host<'a> {
    pub config_path: &'a Path,
    pub tracker: &'a dyn Tracker,
    /// The pass stops taking decisions once this is spent; `None` for none.
    pub budget: Option<Duration>,
    /// Where the action log goes; `None` for the default place.
    pub log_path: Option<PathBuf>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// What a host that embeds the crate hands the bridge for a pass of one of
/// its projects: the repository, whose `.spl-asdlc/config.toml` is read at
/// the default branch, and the instance overlay as a value. Nothing is read
/// from the host's disk.
pub struct RepositoryHost<'a> {
    pub repository: RepoSlug,
    /// The instance overlay's text; `None` when the repository's file
    /// carries every key.
    pub instance: Option<Source>,
    pub tracker: &'a dyn Tracker,
    /// The pass stops taking decisions once this is spent; `None` for none.
    pub budget: Option<Duration>,
    /// Where the action log and its lock go; `None` for the default place.
    pub log_path: Option<PathBuf>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// One pass of a host's project, with the GitHub client the host built
/// (`crate::host::app_github`). A configuration that cannot be read at the
/// default branch (missing, unreadable, invalid) fails the pass, which then
/// writes nothing.
pub fn pass_from_repository(host: &RepositoryHost, github: &dyn GitHub) -> PassOutcome {
    let read = match crate::embed::read_config(github, &host.repository, host.instance.as_ref()) {
        Ok(r) => r,
        Err(e) => return PassOutcome::failed(e.to_string()),
    };
    pass_with(
        &Host {
            config_path: Path::new(crate::config::DEFAULT_PATH),
            tracker: host.tracker,
            budget: host.budget,
            log_path: host.log_path.clone(),
            env: host.env,
        },
        &read.config,
        github,
    )
}

/// One pass with the real GitHub client the configuration asks for.
pub fn pass(host: &Host) -> PassOutcome {
    let config = match Config::load(host.config_path) {
        Ok(c) => c,
        Err(errors) => {
            return PassOutcome::failed(format!(
                "{}: invalid configuration: {}",
                host.config_path.display(),
                errors
                    .0
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    };
    let github = match crate::host::github(&config, host.env) {
        Ok(g) => g,
        Err(e) => return PassOutcome::failed(e),
    };
    pass_with(host, &config, &github)
}

/// One pass with a given GitHub client (tests, and hosts that build their
/// own). `host.config_path` is not read: `config` is the configuration.
pub fn pass_with(host: &Host, config: &Config, github: &dyn GitHub) -> PassOutcome {
    let started = Instant::now();
    let Some(repo) = RepoSlug::parse(&config.product.repository) else {
        return PassOutcome::failed("product.repository is not <owner>/<repo>");
    };
    let path = host
        .log_path
        .clone()
        .or_else(|| ActionLog::default_path(&repo, host.env));
    // One tick at a time (invariant 8), taken before anything is read. A
    // pass with no state directory keeps its log in memory and has no file
    // to lock.
    let _lock = match &path {
        Some(p) => {
            let lock_path = TickLock::beside(p);
            match TickLock::try_acquire(&lock_path) {
                Ok(Some(l)) => Some(l),
                Ok(None) => {
                    return PassOutcome {
                        ok: true,
                        message: crate::lock::already_running(&repo.to_string(), &lock_path),
                    };
                }
                Err(e) => {
                    return PassOutcome::failed(format!(
                        "{repo}: the tick lock {} could not be taken: {e}",
                        lock_path.display()
                    ));
                }
            }
        }
        None => None,
    };
    // The quota first: with almost nothing left, the pass reads nothing
    // and waits for the reset rather than fail on "rate limit exceeded".
    let quota = crate::quota::read(github, &repo);
    if quota.pace() == Pace::Exhausted {
        return PassOutcome {
            ok: true,
            message: format!(
                "{repo}: nothing read, the GitHub quota is almost spent: the next pass after the reset resumes; {}",
                quota.line().unwrap_or_default()
            ),
        };
    }
    let login = match github.identity() {
        Ok(i) => i.login(),
        Err(e) => return PassOutcome::failed(format!("GitHub identity: {e}")),
    };
    let signal = channels(config, host.env);
    let signal = signal.as_ref();
    let t = Tick {
        config,
        repo: repo.clone(),
        github,
        tracker: host.tracker,
        signal,
        github_login: login,
        now: Timestamp::now(),
        deadline: host.budget.map(|b| started + b),
        dry_run: false,
        method_texts: Default::default(),
        low_quota: quota.pace() == Pace::Low,
    };
    let mut log = match path {
        Some(p) => ActionLog::at(p),
        None => ActionLog::in_memory(),
    };
    let report = tick::run(&t, &mut log);
    let flushed = log.flush();
    let mut message = format!("{repo}: {}", report.summary());
    if let Err(e) = &flushed {
        message.push_str(&format!("; the action log could not be written: {e}"));
    }
    if let Some(line) = quota.line() {
        message.push_str(&format!("; {line}"));
    }
    for line in report.waiting_lines() {
        message.push_str(&format!("\n{repo}: {line}"));
    }
    PassOutcome {
        ok: report.ok() && flushed.is_ok(),
        message,
    }
}

/// The notification channels of a configuration. Channels that cannot be
/// built (an unset secret) make every notice they take fail: logged,
/// retried, never marked sent. An event no configured channel takes has
/// nothing to send, built or not.
pub fn channels(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Box<dyn Signal> {
    match Channels::from_config(&config.notify, env) {
        Ok(c) => Box::new(c),
        Err(error) => Box::new(Unavailable {
            error,
            events: config
                .notify
                .iter()
                .flat_map(|n| n.events.iter().cloned())
                .collect(),
        }),
    }
}

struct Unavailable {
    error: String,
    /// The events the configured channels take.
    events: Vec<String>,
}

impl Signal for Unavailable {
    fn signal(&self, event: &str, _text: &str) -> Result<Vec<String>, String> {
        if !self.takes(event) {
            return Ok(Vec::new());
        }
        Err(format!("no channel for `{event}`: {}", self.error))
    }

    fn takes(&self, event: &str) -> bool {
        self.events.iter().any(|e| e == event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Identity;
    use crate::github::fake::FakeGitHub;
    use crate::tracker::fake::FakeTracker;

    /// The example of spec 08 on disk: the repository's file and, beside
    /// it, its instance overlay, which the pass finds by itself.
    fn spec_config() -> (Config, PathBuf) {
        use crate::config::spec_example;
        // One directory per call: the tests run in parallel, and a shared
        // one is rewritten by one test while another reads it.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("spl-asdlc-job-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(crate::config::FILE_NAME);
        std::fs::write(&path, spec_example::repository()).unwrap();
        std::fs::write(
            dir.join(crate::config::INSTANCE_FILE_NAME),
            spec_example::instance(),
        )
        .unwrap();
        (spec_example::config(), path)
    }

    #[test]
    fn a_channel_not_built_fails_its_events_only() {
        let mut config = crate::config::spec_example::config();
        config.notify[0].events = vec!["escalation".into()];
        // The secrets are not on the host: the channel cannot be built.
        let signal = channels(&config, &|_| None);
        assert!(signal.takes("escalation"));
        assert!(signal.signal("escalation", "x").is_err());
        assert!(!signal.takes("release-note"));
        assert!(signal.signal("release-note", "x").unwrap().is_empty());
        config.notify.clear();
        let none = channels(&config, &|_| None);
        assert!(!none.takes("release-note"));
        assert!(none.signal("release-note", "x").unwrap().is_empty());
    }

    fn quiet_github() -> FakeGitHub {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let mut s = g.state.borrow_mut();
        s.identity = Some(Identity::App {
            slug: "spl-asdlc".into(),
            app_id: "1".into(),
        });
        s.default_branch.insert(repo.clone(), "main".into());
        s.tags.insert(repo.clone(), Vec::new());
        s.open_pulls.insert(repo.clone(), Ok(Vec::new()));
        s.merged_pulls.insert(repo, Ok(Vec::new()));
        drop(s);
        g
    }

    #[test]
    fn a_pass_with_nothing_to_do_is_ok_and_leaves_no_file() {
        let (config, path) = spec_config();
        let log = path.with_file_name("quiet.jsonl");
        let _ = std::fs::remove_file(&log);
        let tracker = FakeTracker::default();
        tracker.seed_angle_labels(&config);
        let host = Host {
            config_path: &path,
            tracker: &tracker,
            budget: Some(Duration::from_secs(60)),
            log_path: Some(log.clone()),
            env: &|_| None,
        };
        let out = pass_with(&host, &config, &quiet_github());
        assert!(out.ok, "{out:?}");
        assert!(!log.exists(), "no heartbeat");
        assert!(tracker.writes().is_empty());
    }

    fn quota(remaining: u64) -> crate::github::Read<Vec<crate::github::Quota>> {
        Ok(vec![crate::github::Quota {
            resource: "graphql".into(),
            limit: 5000,
            remaining,
            reset: Timestamp::parse("2026-10-26T13:22:00Z").unwrap(),
        }])
    }

    /// Little left of the quota: the merge pass runs, the sweep of merged
    /// and closed PRs waits, and the line says what is left (ASDLC-5DDN).
    #[test]
    fn a_pass_low_on_quota_skips_the_sweep_and_says_what_is_left() {
        let (config, path) = spec_config();
        let tracker = FakeTracker::default();
        let host = Host {
            config_path: &path,
            tracker: &tracker,
            budget: None,
            log_path: None,
            env: &|_| None,
        };
        let github = quiet_github();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        // A sweep that ran would fail on this read.
        github.state.borrow_mut().merged_pulls.insert(
            repo,
            Err(crate::github::ReadError::unreadable(
                "merged PRs",
                "HTTP 403",
            )),
        );
        github.state.borrow_mut().quota = Some(quota(900));
        let out = pass_with(&host, &config, &github);
        assert!(out.ok, "{out:?}");
        assert!(
            out.message
                .contains("the sweep of merged and closed PRs waits"),
            "{}",
            out.message
        );
        assert!(
            out.message
                .contains("GitHub quota: graphql 900/5000 until 13:22Z"),
            "{}",
            out.message
        );
        github.state.borrow_mut().quota = Some(quota(4000));
        let out = pass_with(&host, &config, &github);
        assert!(!out.ok, "with the quota back, the sweep runs: {out:?}");
        assert!(
            out.message.contains("GitHub quota: graphql 4000/5000"),
            "{}",
            out.message
        );
    }

    /// Almost nothing left: the pass reads nothing (its GitHub knows no
    /// identity, so a pass that went on would fail) and waits for the reset.
    #[test]
    fn a_pass_with_the_quota_spent_reads_nothing_and_is_no_failure() {
        let (config, path) = spec_config();
        let tracker = FakeTracker::default();
        let github = FakeGitHub::new();
        github.state.borrow_mut().quota = Some(quota(100));
        let out = pass_with(
            &Host {
                config_path: &path,
                tracker: &tracker,
                budget: None,
                log_path: None,
                env: &|_| None,
            },
            &config,
            &github,
        );
        assert!(out.ok, "{out:?}");
        assert!(out.message.contains("nothing read"), "{}", out.message);
        assert!(out.message.contains("until 13:22Z"), "{}", out.message);
        assert!(tracker.writes().is_empty());
        assert!(github.writes().is_empty());
    }

    /// A tick already running (a timer and a run by hand overlapping): the
    /// second pass reads nothing, writes nothing, and says why. Its GitHub
    /// knows no identity, so a pass that went on would fail.
    #[cfg(unix)]
    #[test]
    fn a_second_tick_while_the_lock_is_held_does_nothing() {
        let (config, path) = spec_config();
        let log = path.with_file_name("locked.jsonl");
        let _ = std::fs::remove_file(&log);
        let held = TickLock::try_acquire(&TickLock::beside(&log))
            .unwrap()
            .expect("the first tick's lock");
        let tracker = FakeTracker::default();
        let host = Host {
            config_path: &path,
            tracker: &tracker,
            budget: None,
            log_path: Some(log.clone()),
            env: &|_| None,
        };
        let github = FakeGitHub::new();
        let out = pass_with(&host, &config, &github);
        assert!(out.ok, "{out:?}");
        assert!(
            out.message.contains("a tick is already running"),
            "{}",
            out.message
        );
        assert!(tracker.writes().is_empty());
        assert!(github.writes().is_empty());
        assert!(!log.exists(), "no log line");

        drop(held);
        let out = pass_with(&host, &config, &github);
        assert!(!out.ok, "the lock released, the pass runs: {out:?}");
        assert!(out.message.contains("GitHub identity"), "{}", out.message);
    }

    /// A host hands the bridge the repository's file only: the overlay
    /// beside it is merged, so the pass goes as far as the credentials.
    #[test]
    fn the_pass_reads_the_instance_overlay_beside_the_configuration() {
        let (_, path) = spec_config();
        let tracker = FakeTracker::default();
        let out = pass(&Host {
            config_path: &path,
            tracker: &tracker,
            budget: None,
            log_path: None,
            env: &|_| None,
        });
        assert!(!out.ok);
        assert!(
            !out.message.contains("invalid configuration") && out.message.contains("SPL_ASDLC_APP"),
            "{}",
            out.message
        );
    }

    #[test]
    fn an_invalid_configuration_fails_the_pass_with_its_reasons() {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-job-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(crate::config::FILE_NAME);
        std::fs::write(&path, "schema = 1\n").unwrap();
        let tracker = FakeTracker::default();
        let out = pass(&Host {
            config_path: &path,
            tracker: &tracker,
            budget: None,
            log_path: None,
            env: &|_| None,
        });
        assert!(!out.ok);
        assert!(
            out.message.contains("invalid configuration"),
            "{}",
            out.message
        );
    }

    #[test]
    fn a_spent_budget_stops_the_pass_and_fails_it() {
        let (config, path) = spec_config();
        let tracker = FakeTracker::default();
        let host = Host {
            config_path: &path,
            tracker: &tracker,
            budget: Some(Duration::ZERO),
            log_path: Some(path.with_file_name("spent.jsonl")),
            env: &|_| None,
        };
        let out = pass_with(&host, &config, &quiet_github());
        assert!(!out.ok);
        assert!(out.message.contains("time budget"), "{}", out.message);
    }

    fn repository_host(tracker: &FakeTracker) -> RepositoryHost<'_> {
        RepositoryHost {
            repository: RepoSlug::parse("example-org/acme").unwrap(),
            instance: Some(Source {
                label: "the host's instance values".into(),
                text: crate::config::spec_example::instance().into(),
            }),
            tracker,
            budget: Some(Duration::from_secs(60)),
            log_path: None,
            env: &|_| None,
        }
    }

    fn with_file(github: &FakeGitHub, text: &str) {
        github.state.borrow_mut().files.insert(
            (
                RepoSlug::parse("example-org/acme").unwrap(),
                "main".into(),
                crate::config::DEFAULT_PATH.into(),
            ),
            Ok(text.into()),
        );
    }

    /// The configuration read at the default branch through the App,
    /// merged with the overlay the host passes: nothing on the host's disk.
    #[test]
    fn a_host_runs_a_pass_from_the_repositorys_file_and_an_overlay_value() {
        let github = quiet_github();
        with_file(&github, crate::config::spec_example::repository());
        let tracker = FakeTracker::default();
        tracker.seed_angle_labels(&crate::config::spec_example::config());
        let out = pass_from_repository(&repository_host(&tracker), &github);
        assert!(out.ok, "{out:?}");
        assert!(tracker.writes().is_empty());
        assert!(github.writes().is_empty());
    }

    #[test]
    fn a_missing_unreadable_or_invalid_file_fails_the_pass_and_writes_nothing() {
        let tracker = FakeTracker::default();
        let missing = quiet_github();
        let out = pass_from_repository(&repository_host(&tracker), &missing);
        assert!(!out.ok);
        assert!(
            out.message
                .contains("example-org/acme:.spl-asdlc/config.toml@main: no such file"),
            "{}",
            out.message
        );

        let unreadable = quiet_github();
        unreadable.state.borrow_mut().files.insert(
            (
                RepoSlug::parse("example-org/acme").unwrap(),
                "main".into(),
                crate::config::DEFAULT_PATH.into(),
            ),
            Err(crate::github::ReadError::Unreadable {
                what: "the file".into(),
                status: Some(502),
                detail: "bad gateway".into(),
            }),
        );
        let out = pass_from_repository(&repository_host(&tracker), &unreadable);
        assert!(!out.ok);
        assert!(out.message.contains("could not be read"), "{}", out.message);

        let invalid = quiet_github();
        with_file(&invalid, "schema = 1\n");
        let out = pass_from_repository(&repository_host(&tracker), &invalid);
        assert!(!out.ok);
        assert!(
            out.message.contains("invalid configuration"),
            "{}",
            out.message
        );
        for g in [&missing, &unreadable, &invalid] {
            assert!(g.writes().is_empty());
        }
        assert!(tracker.writes().is_empty());
    }

    /// The merge of the files on disk: a key in both is an error naming
    /// both sources, an instance key in the repository's file is refused.
    #[test]
    fn the_merge_follows_the_rules_of_the_files_on_disk() {
        let tracker = FakeTracker::default();
        let both = quiet_github();
        let repository = crate::config::spec_example::repository().replacen(
            "[product]\n",
            "[product]\nrepository = \"example-org/acme\"\n",
            1,
        );
        with_file(&both, &repository);
        let out = pass_from_repository(&repository_host(&tracker), &both);
        assert!(!out.ok);
        assert!(
            out.message.contains("product.repository")
                && out.message.contains("the host's instance values"),
            "{}",
            out.message
        );

        // Without an overlay the same file is read alone, its instance keys
        // its own; another repository's file is refused.
        let mut host = repository_host(&tracker);
        host.instance = None;
        host.repository = RepoSlug::parse("example-org/other").unwrap();
        let other = quiet_github();
        other
            .state
            .borrow_mut()
            .default_branch
            .insert(host.repository.clone(), "main".into());
        other.state.borrow_mut().files.insert(
            (
                host.repository.clone(),
                "main".into(),
                crate::config::DEFAULT_PATH.into(),
            ),
            Ok(include_str!("../docs/setup/templates/config.toml").into()),
        );
        let out = pass_from_repository(&host, &other);
        assert!(!out.ok);
        assert!(
            out.message
                .contains("not the repository the file was read from"),
            "{}",
            out.message
        );
    }
}
