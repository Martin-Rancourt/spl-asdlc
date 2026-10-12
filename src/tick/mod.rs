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
//!
//! An item or PR under the hands-off label is written to by no pass
//! ([`hands_off`]): [`run`] wraps the tracker and GitHub in its guard, and
//! [`Tick::act`] reports a write the guard refused as hands-off
//! ([`Report::hands_off`]), neither done nor failed.

pub mod closure;
pub mod hands_off;
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
    /// The hands-off guard [`run`] puts in front of `tracker` and `github`;
    /// `None` outside a tick (a verb is a person's own gesture).
    pub hands_off: Option<&'a hands_off::HandsOff<'a>>,
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
    /// The items and PRs under the hands-off label the tick met and wrote
    /// nothing to (`ABC-12`, `pr#7`), each once: said at every tick, noted
    /// in the action log only when it starts.
    pub hands_off: Vec<String>,
}

/// What became of a notice ([`Tick::notify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// Sent through every channel that takes its event.
    Sent,
    /// No configured channel takes its event: nothing to send.
    NoChannel,
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

    /// One line per object under the hands-off label: `hands-off: <object>`.
    pub fn hands_off_lines(&self) -> Vec<String> {
        self.hands_off
            .iter()
            .map(|o| format!("hands-off: {o}"))
            .collect()
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
    /// reported as a failure), or when the hands-off guard refused it: then
    /// nothing is logged, the object is reported once as hands-off, and the
    /// decision stops there as after a failure, without being one.
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
        if let Some(guard) = self.hands_off {
            let refused = guard.take_refused();
            if result.is_err() && !refused.is_empty() {
                for object in refused {
                    self.hold_by_hand(report, &object);
                }
                return None;
            }
        }
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

    /// Send a notice through the channels that take `event` (spec 07,
    /// "Notices"). No configured channel takes it: nothing to send, which
    /// is not a failure and writes nothing here (the caller's tracker or PR
    /// comment is the record, and marks the notice done). A channel that
    /// takes it and fails is a failure: logged, retried at the next tick.
    /// `None` when it failed.
    pub fn notify(
        &self,
        log: &mut ActionLog,
        report: &mut Report,
        object: &str,
        event: &str,
        inputs: Value,
        text: &str,
    ) -> Option<Notice> {
        if !self.signal.takes(event) {
            return Some(Notice::NoChannel);
        }
        self.act(
            log,
            report,
            object,
            &format!("notify {event}"),
            inputs,
            || self.signal.signal(event, text).map(|_| ()),
        )
        .map(|()| Notice::Sent)
    }

    /// An object under the hands-off label: in the report once per tick,
    /// nothing written.
    pub fn hold_by_hand(&self, report: &mut Report, object: &str) {
        if !report.hands_off.iter().any(|o| o == object) {
            report.hands_off.push(object.to_string());
        }
    }

    /// Whether the PR carries the hands-off label; when it does, it is
    /// reported as hands-off and the caller leaves it alone: no merge, no
    /// comment, no review round, nothing moved because of it.
    pub fn by_hand(&self, report: &mut Report, pr: &PullRequest) -> bool {
        if !hands_off::carries(&pr.labels, &self.config.bridge.hands_off_label) {
            return false;
        }
        self.hold_by_hand(report, &hands_off::pr_object(pr.number));
        true
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
    // Every read and write of the passes goes through the hands-off guard.
    let guard = hands_off::HandsOff::new(&t.config.bridge.hands_off_label, t.tracker, t.github);
    let guarded = Tick {
        config: t.config,
        repo: t.repo.clone(),
        github: &guard,
        tracker: &guard,
        signal: t.signal,
        github_login: t.github_login.clone(),
        now: t.now,
        deadline: t.deadline,
        dry_run: t.dry_run,
        hands_off: Some(&guard),
    };
    let t = &guarded;
    let mut report = Report::default();
    passes(t, log, &mut report);
    // Why each held PR waits is in the report at every tick; the log keeps
    // it only when it changes (a dry run keeps nothing).
    if !t.dry_run {
        for w in &report.waiting {
            log.note_waiting(t.now, &t.repo, &w.object(), &w.head, &w.reason);
        }
        for object in &report.hands_off {
            log.note_hands_off(t.now, &t.repo, object, guard.label());
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
