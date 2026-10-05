//! One pass of the bridge for a host (`docs/spec/07-the-bridge.md`
//! invariant 1): the Livela runner's daemon job, a timer, a shell. The host
//! gives the configuration file, the tracker and a time budget; the pass
//! reads its secrets from the environment, runs one tick, appends the action
//! log, and answers ok or failed with one line for the host's job log.
//!
//! A pass that does nothing writes nothing: no fact, no log line, no file
//! (invariant 7). Only the host records a failed pass.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::action_log::ActionLog;
use crate::config::Config;
use crate::github::{GitHub, RepoSlug, Timestamp};
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
    let path = host
        .log_path
        .clone()
        .or_else(|| ActionLog::default_path(&repo, host.env));
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
    PassOutcome {
        ok: report.ok() && flushed.is_ok(),
        message,
    }
}

/// The notification channels of a configuration. Channels that cannot be
/// built (an unset secret) make every notice fail: logged, retried, never
/// marked sent.
pub fn channels(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Box<dyn Signal> {
    match Channels::from_config(&config.notify, env) {
        Ok(c) => Box::new(c),
        Err(e) => Box::new(Unavailable(e)),
    }
}

struct Unavailable(String);

impl Signal for Unavailable {
    fn signal(&self, event: &str, _text: &str) -> Result<Vec<String>, String> {
        Err(format!("no channel for `{event}`: {}", self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Identity;
    use crate::github::fake::FakeGitHub;
    use crate::tracker::fake::FakeTracker;

    fn spec_config() -> (Config, PathBuf) {
        let spec = include_str!("../docs/spec/08-configuration.md");
        let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
        let end = start + spec[start..].find("```").unwrap();
        let text = &spec[start..end];
        let dir = std::env::temp_dir().join(format!("spl-asdlc-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spl-asdlc.toml");
        std::fs::write(&path, text).unwrap();
        (Config::parse(text).unwrap(), path)
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
