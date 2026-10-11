//! One pass of the bridge for a host (`docs/spec/07-the-bridge.md`
//! invariant 1): the Livela runner's daemon job, a timer, a shell. The host
//! gives the configuration file, the tracker and a time budget; the pass
//! reads its secrets from the environment, runs one tick, appends the action
//! log, and answers ok or failed with one line for the host's job log.
//!
//! A pass that does nothing writes nothing: no fact, no log line, no file
//! (invariant 7) but the empty lock file beside the action log, which keeps
//! a second pass on the same repository from running while one runs
//! (invariant 8): that second pass does nothing and says so. Only the host
//! records a failed pass.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::action_log::ActionLog;
use crate::config::Config;
use crate::github::{GitHub, RepoSlug, Timestamp};
use crate::lock::TickLock;
use crate::notify::{Channels, Signal};
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
/// own).
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
        let dir = std::env::temp_dir().join(format!("spl-asdlc-job-{}", std::process::id()));
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
        let path = dir.join("spl-asdlc.toml");
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
}
