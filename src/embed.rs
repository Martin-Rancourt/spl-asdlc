//! The bridge embedded in a host application (`docs/reference/livela-contract.md`,
//! "A host with several projects"): the host runs it for one of its
//! projects from this library alone. The configuration is the repository's
//! `.spl-asdlc/config.toml` read at its default branch through the App,
//! merged with an instance overlay the host passes as a value; nothing is
//! read from the host's disk, and no CLI or second tracker connection is
//! needed. The host passes its own `Tracker` (the Livela adapter, scoped to
//! the project) and GitHub client.
//!
//! What the host calls: [`read_config`], then [`crate::job::pass_from_repository`]
//! for a pass, [`Project::doctor`] for the setup checks, the gestures of
//! [`Project`] for a person's acts, and [`crate::setup::files`] for the
//! files of the project's setup PR.

use std::path::PathBuf;

use serde::Serialize;

use crate::action_log::ActionLog;
use crate::config::{Config, DEFAULT_PATH, Source};
use crate::doctor::Check;
use crate::github::{GitHub, RepoSlug, Timestamp};
use crate::tick::Tick;
use crate::tracker::Tracker;
use crate::verbs::{self, CycleStatus};

/// The version of this crate, the one the host embeds.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The minor line of a version (`1.4.2` → `v1.4`): the ref the callers of a
/// host's projects pin, moved at each release of the line.
pub fn minor_line(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("v{major}.{minor}"),
        _ => format!("v{version}"),
    }
}

/// Why the repository's configuration could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum ReadError {
    /// The repository or its default branch could not be read.
    Repository { repository: String, error: String },
    /// No `.spl-asdlc/config.toml` at the default branch.
    Missing { source: String },
    /// The file is there but could not be read.
    Unreadable { source: String, error: String },
    /// The file, or its merge with the overlay, is invalid.
    Invalid { source: String, errors: Vec<String> },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Repository { repository, error } => {
                write!(
                    f,
                    "{repository}: the default branch could not be read: {error}"
                )
            }
            ReadError::Missing { source } => write!(f, "{source}: no such file"),
            ReadError::Unreadable { source, error } => {
                write!(f, "{source} could not be read: {error}")
            }
            ReadError::Invalid { source, errors } => {
                write!(f, "{source}: invalid configuration: {}", errors.join("; "))
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// A project's configuration as the bridge reads it for a host.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub config: Config,
    pub repo: RepoSlug,
    /// The branch the file was read at.
    pub default_branch: String,
    /// What names the file read in a message (`<owner>/<repo>:.spl-asdlc/config.toml@main`).
    pub source: String,
    /// The instance overlay merged into it, if any.
    pub instance: Option<Source>,
}

/// Read `.spl-asdlc/config.toml` at the repository's default branch through
/// `github` and merge it with `instance`, by the rules of the files on disk
/// (spec 08): a key set in both is an error naming both, an instance key in
/// the repository's file is refused. The merged `product.repository` must
/// be the repository read. Reads only.
pub fn read_config(
    github: &dyn GitHub,
    repo: &RepoSlug,
    instance: Option<&Source>,
) -> Result<ProjectConfig, ReadError> {
    let default_branch = github
        .default_branch(repo)
        .map_err(|e| ReadError::Repository {
            repository: repo.to_string(),
            error: e.to_string(),
        })?;
    let source = format!("{repo}:{DEFAULT_PATH}@{default_branch}");
    let text = match github.file(repo, DEFAULT_PATH, &default_branch) {
        Ok(text) => text,
        Err(e) if e.is_not_found() => return Err(ReadError::Missing { source }),
        Err(e) => {
            return Err(ReadError::Unreadable {
                source,
                error: e.to_string(),
            });
        }
    };
    let file = Source {
        label: source.clone(),
        text,
    };
    let config = Config::parse_with(&file, instance).map_err(|errors| ReadError::Invalid {
        source: source.clone(),
        errors: errors.0.iter().map(|e| e.to_string()).collect(),
    })?;
    if config.product.repository != repo.to_string() {
        return Err(ReadError::Invalid {
            source,
            errors: vec![format!(
                "product.repository is {}, not the repository the file was read from",
                config.product.repository
            )],
        });
    }
    Ok(ProjectConfig {
        config,
        repo: repo.clone(),
        default_branch,
        source,
        instance: instance.cloned(),
    })
}

/// What a gesture did, or why it was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// `release cut`, `release abandon`, …
    pub gesture: String,
    pub done: bool,
    /// What was done, or why nothing was.
    pub message: String,
}

impl Outcome {
    fn of(gesture: &str, result: Result<String, String>) -> Outcome {
        let (done, message) = match result {
            Ok(m) => (true, m),
            Err(m) => (false, m),
        };
        Outcome {
            gesture: gesture.to_string(),
            done,
            message,
        }
    }
}

/// What `release status` says, for a host.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "cycle", rename_all = "kebab-case")]
pub enum ReleaseStatus {
    /// A cycle is in flight: its release item, its candidate and its GO
    /// (`decision`, the GO's inputs as `release status --json` prints them).
    InFlight {
        item: String,
        candidate: String,
        go: bool,
        reasons: Vec<String>,
        blockers_carried: String,
        decision: serde_json::Value,
    },
    /// The newest candidate's cycle closed with this release.
    Over { release_tag: String, item: String },
}

impl ReleaseStatus {
    fn of(status: CycleStatus) -> ReleaseStatus {
        match status {
            CycleStatus::Over { release_tag, item } => ReleaseStatus::Over { release_tag, item },
            CycleStatus::InFlight(cycle, d) => ReleaseStatus::InFlight {
                item: cycle.item.key.clone(),
                candidate: cycle.candidate.tag.clone(),
                go: d.go,
                reasons: d.reasons.clone(),
                blockers_carried: d.blockers_carried(),
                decision: d.as_json(),
            },
        }
    }

    /// The JSON `release status --json` prints.
    pub fn as_json(&self) -> serde_json::Value {
        match self {
            ReleaseStatus::InFlight { decision, .. } => decision.clone(),
            ReleaseStatus::Over { release_tag, item } => {
                serde_json::json!({"in_flight": false, "closed_with": release_tag, "release_item": item})
            }
        }
    }
}

/// One project of a host, for its gestures and its `doctor`: the
/// configuration, the two clients and where the action log goes. The CLI
/// builds the same and calls the same functions.
pub struct Project<'a> {
    pub config: &'a Config,
    pub repo: RepoSlug,
    pub github: &'a dyn GitHub,
    pub tracker: &'a dyn Tracker,
    /// Where the action log goes; `None` for the default place.
    pub log_path: Option<PathBuf>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

impl<'a> Project<'a> {
    /// The project of a configuration; refused when its repository is not
    /// `<owner>/<repo>`.
    pub fn new(
        config: &'a Config,
        github: &'a dyn GitHub,
        tracker: &'a dyn Tracker,
        log_path: Option<PathBuf>,
        env: &'a dyn Fn(&str) -> Option<String>,
    ) -> Result<Project<'a>, String> {
        let repo = RepoSlug::parse(&config.product.repository)
            .ok_or("product.repository is not <owner>/<repo>")?;
        Ok(Project {
            config,
            repo,
            github,
            tracker,
            log_path,
            env,
        })
    }

    /// `doctor` for a host that embeds this crate: every check of the
    /// procedure, and whether the workflow callers name the minor line of
    /// the embedded version. `host` names the host in the report.
    pub fn doctor(&self, instance: Option<&Source>, host: &str) -> Vec<Check> {
        crate::doctor::run_embedded(
            self.config,
            instance,
            self.github,
            self.tracker,
            host,
            Some(VERSION),
        )
    }

    /// `release status`: the cycle in flight and its GO, read-only.
    pub fn release_status(&self) -> Result<ReleaseStatus, String> {
        let signal = crate::job::channels(self.config, self.env);
        let t = self.tick(signal.as_ref())?;
        verbs::release_status(&t).map(ReleaseStatus::of)
    }

    /// `release cut [--open]`, by `who`.
    pub fn cut(&self, open: bool, who: &str) -> Outcome {
        self.gesture("release cut", |t, log| verbs::cut(t, log, open, who))
    }

    /// `release requalify <ticket> --blocking <b> --reason <r>`, by `who`.
    pub fn requalify(&self, ticket: &str, blocking: bool, reason: &str, who: &str) -> Outcome {
        self.gesture("release requalify", |t, log| {
            verbs::requalify(t, log, ticket, blocking, reason, who)
        })
    }

    /// `release abandon`, by `who`.
    pub fn abandon(&self, who: &str) -> Outcome {
        self.gesture("release abandon", |t, log| verbs::abandon(t, log, who))
    }

    /// `release retract <tag>`, by `who`.
    pub fn retract(&self, tag: &str, who: &str) -> Outcome {
        self.gesture("release retract", |t, log| verbs::retract(t, log, tag, who))
    }

    /// A gesture through [`Tick::act`], its action log flushed after it,
    /// done or refused.
    fn gesture(
        &self,
        name: &str,
        f: impl FnOnce(&Tick, &mut ActionLog) -> Result<String, String>,
    ) -> Outcome {
        let signal = crate::job::channels(self.config, self.env);
        let t = match self.tick(signal.as_ref()) {
            Ok(t) => t,
            Err(e) => return Outcome::of(name, Err(e)),
        };
        let mut log = match self
            .log_path
            .clone()
            .or_else(|| ActionLog::default_path(&self.repo, self.env))
        {
            Some(p) => ActionLog::at(p),
            None => ActionLog::in_memory(),
        };
        let result = f(&t, &mut log);
        let result = match (result, log.flush()) {
            (Ok(m), Err(e)) => Ok(format!("{m}; the action log could not be written: {e}")),
            (r, _) => r,
        };
        Outcome::of(name, result)
    }

    fn tick<'s>(&'s self, signal: &'s dyn crate::notify::Signal) -> Result<Tick<'s>, String> {
        let login = self
            .github
            .identity()
            .map_err(|e| format!("GitHub identity: {e}"))?
            .login();
        Ok(Tick {
            config: self.config,
            repo: self.repo.clone(),
            github: self.github,
            tracker: self.tracker,
            signal,
            github_login: login,
            now: Timestamp::now(),
            deadline: None,
            dry_run: false,
            method_texts: Default::default(),
            low_quota: false,
        })
    }
}

#[cfg(test)]
mod tests;
