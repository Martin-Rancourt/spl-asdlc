//! `spl-asdlc tick` (`docs/spec/07-the-bridge.md`): one pass over one
//! repository. It reads GitHub and the tracker, decides with the method
//! (`crate::method`), writes the effects, and records each action in the
//! action log.
//!
//! Every write goes through [`Tick::act`]: it is logged with the facts it
//! rested on, a failure is logged and retried at the next tick, and a dry run
//! records the action as what it would do. A decision whose effect is
//! already in place calls no write and leaves no entry: a tick with nothing
//! to do writes nothing, anywhere (invariant 7), but the action log's note
//! of why a PR waits when that reason changes ([`Report::waiting`]).

pub mod closure;
pub mod link;
pub mod merge;
pub mod post_deploy;
pub mod release;
pub mod sweep;

#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod tests;

use std::time::Instant;

use serde_json::Value;

use crate::action_log::{ActionLog, Outcome};
use crate::config::Config;
use crate::github::{GitHub, PullRequest, RepoSlug, Timestamp};
use crate::notify::Signal;
use crate::tracker::Tracker;

/// What one tick works with.
pub struct Tick<'a> {
    pub config: &'a Config,
    pub repo: RepoSlug,
    pub github: &'a dyn GitHub,
    pub tracker: &'a dyn Tracker,
    pub signal: &'a dyn Signal,
    /// The login GitHub shows on the bridge's own comments: markers are read
    /// only there.
    pub github_login: String,
    pub now: Timestamp,
    /// The tick stops taking new decisions past this instant (a host's
    /// budget); what it skipped is taken at the next tick.
    pub deadline: Option<Instant>,
    pub dry_run: bool,
}

/// What a tick did, for its host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Actions taken (or, in a dry run, that would be taken).
    pub actions: usize,
    /// Actions that failed, and decisions that could not be taken because a
    /// read failed: each is retried at the next tick.
    pub failures: Vec<String>,
    /// The deadline cut the tick short.
    pub out_of_time: bool,
    /// The PRs the merge pass holds, each with why (spec 07): said at every
    /// tick, written nowhere but the action log, and there only when the
    /// reason changes.
    pub waiting: Vec<Waiting>,
}

/// A PR the merge pass holds, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub pr: u64,
    pub head: String,
    pub reason: String,
}

impl Waiting {
    /// What the action log is about.
    pub fn object(&self) -> String {
        format!("pr#{}", self.pr)
    }
}

impl std::fmt::Display for Waiting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "waiting: pr#{} {}", self.pr, self.reason)
    }
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failures.is_empty() && !self.out_of_time
    }

    /// One line per PR the merge pass holds: `waiting: pr#<n> <reason>`.
    pub fn waiting_lines(&self) -> Vec<String> {
        self.waiting.iter().map(Waiting::to_string).collect()
    }

    /// One line for a host's job log.
    pub fn summary(&self) -> String {
        let mut s = format!("{} action(s)", self.actions);
        if !self.failures.is_empty() {
            s.push_str(&format!(
                ", {} failure(s): {}",
                self.failures.len(),
                self.failures.join("; ")
            ));
        }
        if self.out_of_time {
            s.push_str(", stopped at the time budget");
        }
        s
    }
}

impl Tick<'_> {
    /// Whether the tick may still take a decision.
    pub fn in_time(&self) -> bool {
        self.deadline.is_none_or(|d| Instant::now() < d)
    }

    /// Perform one write and log it. `None` when it failed (logged, and
    /// reported as a failure).
    pub fn act<T>(
        &self,
        log: &mut ActionLog,
        report: &mut Report,
        object: &str,
        action: &str,
        inputs: Value,
        write: impl FnOnce() -> Result<T, String>,
    ) -> Option<T> {
        let result = write();
        let outcome = match &result {
            Ok(_) if self.dry_run => Outcome::WouldDo,
            Ok(_) => Outcome::Done,
            Err(e) => Outcome::Failed(e.clone()),
        };
        log.record(self.now, &self.repo, object, action, inputs, outcome);
        match result {
            Ok(v) => {
                report.actions += 1;
                Some(v)
            }
            Err(e) => {
                report.failures.push(format!("{object}: {action}: {e}"));
                None
            }
        }
    }

    /// A decision that could not be taken because a read failed: nothing is
    /// written, the failure is reported, the next tick retries (fail closed).
    pub fn unreadable(&self, report: &mut Report, object: &str, detail: impl std::fmt::Display) {
        report.failures.push(format!("{object}: {detail}"));
    }

    /// A PR the merge pass holds: why goes in the report, nothing is
    /// written to GitHub or the tracker. Said once per PR and tick (the
    /// first reason found).
    pub fn wait(&self, report: &mut Report, pr: &PullRequest, reason: impl Into<String>) {
        if report.waiting.iter().any(|w| w.pr == pr.number) {
            return;
        }
        report.waiting.push(Waiting {
            pr: pr.number,
            head: pr.head_sha.clone(),
            reason: reason.into(),
        });
    }
}

/// One tick over the repository of `t.config`: the release pass (with its
/// closure; nothing without a `[release]` section, spec 05 "Without a
/// release cycle"), the PR pass, the sweep, the post-deploy validation
/// (spec 07, "What a tick does"). At most one merge per
/// repository per tick (invariant 6). The caller flushes the action log.
pub fn run(t: &Tick, log: &mut ActionLog) -> Report {
    let mut report = Report::default();
    passes(t, log, &mut report);
    // Why each held PR waits is in the report at every tick; the log keeps
    // it only when it changes (a dry run keeps nothing).
    if !t.dry_run {
        for w in &report.waiting {
            log.note_waiting(t.now, &t.repo, &w.object(), &w.head, &w.reason);
        }
    }
    report
}

fn passes(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    if !t.in_time() {
        report.out_of_time = true;
        return;
    }
    let merged = release::pass(t, log, report);
    if !t.in_time() {
        report.out_of_time = true;
        return;
    }
    let pass = merge::pass(t, log, report);
    let mut merged_now = merged;
    // The first candidate GitHub accepts; a refused merge does not starve
    // the others.
    for c in &pass.candidates {
        if merged_now {
            t.wait(
                report,
                &c.pr,
                "ready, but a PR was merged this tick: one merge per repository per tick",
            );
            continue;
        }
        if !t.in_time() {
            report.out_of_time = true;
            break;
        }
        if merge::merge(t, log, report, c, &pass.open) {
            merged_now = true;
        }
    }
    if t.in_time() {
        sweep::pass(t, log, report, &pass.open);
    } else {
        report.out_of_time = true;
        return;
    }
    if t.in_time() {
        post_deploy::pass(t, log, report);
    } else {
        report.out_of_time = true;
    }
}
